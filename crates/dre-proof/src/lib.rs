//! Phase 10 — Proof engine.
//!
//! Every decision must be explainable, so every reasoner records what it
//! did into a proof graph as it goes.
//!
//! The important design choice is that rejected candidates are recorded
//! alongside accepted ones. A proof that shows only the winning path
//! answers "what did it conclude" but not "why not the alternative", and
//! the second question is usually the one being asked when someone
//! distrusts a result.
//!
//! Nodes are appended in engine execution order and referenced by index,
//! so the graph is a deterministic function of the run. The `ProofHash`
//! covers the whole structure: two runs agreeing on a decision but
//! reaching it differently are a determinism failure, and comparing only
//! `DecisionHash` would hide that.
//!
//! # Provenance
//!
//! [`ProofNode::inputs`] is what turns a list of steps into a graph. Without
//! it a proof answers "what happened" in order, but not "what did *this*
//! step rest on" — and that second question is the one asked when a result
//! is disputed. A conclusion has to be traceable past the rule that drew it,
//! back to the measurements the rule read, and on to the sources that
//! outvoted the ones that disagreed.
//!
//! Edges are built by [`Trace`], which is the graph plus a note of which
//! node most recently established each subject. The map is deliberately not
//! a field of [`ProofGraph`]: the graph is the artefact that gets hashed,
//! serialised and compared, and it would then carry build-time scaffolding
//! that vanished on a round trip while equality claimed the two were the
//! same. Keeping them separate keeps the hashed artefact honest.

pub mod render;

pub use render::{explain, render, summarize};

use dre_core::collections::DetMap;

use dre_core::canonical::{Canonical, CanonicalWriter};
use dre_core::fixed::Fixed;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};

/// Index of a node within a [`ProofGraph`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeRef(pub u32);

impl Canonical for NodeRef {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("node-ref").write_u64(u64::from(self.0));
    }
}

/// What a proof node records.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// An input fact entering the engine.
    Input {
        /// Property the fact concerns.
        subject: Id,
    },
    /// A rule whose conditions were evaluated.
    RuleEvaluated {
        /// Rule identifier.
        rule: Id,
        /// Whether the conditions held.
        fired: bool,
    },
    /// A value derived by a rule.
    Inference {
        /// Property that was inferred.
        subject: Id,
    },
    /// A declared feature evaluated.
    ///
    /// Recorded whether or not it produced a value. A feature that came
    /// back `UNKNOWN` or `INVALID` is part of the reasoning — it says the
    /// engine looked and could not derive the quantity, which is a
    /// different statement from never having asked.
    Feature {
        /// Feature identifier.
        feature: Id,
        /// Property it writes.
        subject: Id,
        /// Whether it produced a value.
        resolved: bool,
    },
    /// A geometric computation.
    Geometry {
        /// Operator applied.
        operator: Id,
    },
    /// A candidate result produced by a reasoner.
    Candidate {
        /// Candidate identifier.
        candidate: Id,
    },

    /// An observation that *backed* a resolution.
    ///
    /// The counterpart to [`NodeKind::Rejected`], and it was missing. A
    /// contested measurement recorded the reading it overruled and
    /// collapsed the readings that won into a single anonymous input, so
    /// the proof preserved provenance for the discarded observation and
    /// discarded it for the decisive ones.
    ///
    /// Found by the third independently authored pack, diagnosing a switch
    /// whose whole question was which instrument to trust: *"I would prefer
    /// the proof to preserve explicit provenance for the observations that
    /// won, not only the observation that lost."*
    ///
    /// The source is carried as well as the identifier, because "which
    /// reading" and "from which instrument" are different questions and
    /// only the second one survives being asked a week later.
    Observed {
        /// The observation's identifier.
        observation: Id,
        /// The source it came from.
        source: Id,
    },
    /// An observation that was attempted and produced no value.
    ///
    /// Distinct from [`NodeKind::Rejected`] on purpose. A rejected reading
    /// arrived and was overruled or refused; this one never arrived at all,
    /// and recording both the same way would repeat exactly the conflation
    /// the independent author objected to — that a value which failed to
    /// exist is not a value that lost.
    ObservationFailed {
        /// The attempt's identifier.
        observation: Id,
        /// The instrument that did not answer.
        source: Id,
        /// Why no value came back.
        reason: String,
    },
    /// One declared hypothesis, scored against the observed state.
    ///
    /// Recorded for **every** hypothesis, not only the winner, and with the
    /// components rather than just the total. "Why POWER and not THERMAL"
    /// is the entire question an explanation layer exists to answer, and a
    /// proof carrying one number could not answer it: two hypotheses can
    /// reach the same score by explaining a lot and contradicting a lot, or
    /// by explaining little and contradicting nothing, and those are not
    /// the same finding.
    ///
    /// `complexity` is left out because it is a property of the pack rather
    /// than of the run -- it is the mechanism count, readable from the
    /// section, and the `ModelHash` already covers it.
    Explanation {
        /// The hypothesis's identifier.
        hypothesis: Id,
        /// The total it was ranked on.
        score: Fixed,
        /// Explanatory credit earned, corroboration already discounted.
        coverage: Fixed,
        /// Mechanisms the state backs.
        supported: u32,
        /// Observations it predicted and the world denied.
        contradicted: u32,
        /// Observations that are happening and it does not account for.
        unexplained: u32,
        /// Mechanisms it posits that nothing supports.
        assumptions: u32,
    },
    /// A traversal along declared relationships.
    ///
    /// Recorded because a conclusion that rests on a traversal has to be
    /// answerable for where it went. Without it the proof says a rule fired
    /// and gives no account of which entities it reached — the same gap
    /// `NodeKind::Observed` was added in 0.8.0 to close, where the proof kept
    /// provenance for the reading that lost and dropped it for the ones that
    /// decided the answer.
    ///
    /// Every field is an owned type with an explicitly written tag. A
    /// `&'static str` cannot deserialize and `NodeKind` derives
    /// `Deserialize`; and a hop is two fields rather than an `"out:mounts"`
    /// string, so no separator is smuggled into the canonical encoding.
    Reach {
        /// Where the traversal started, after binding.
        anchor: Id,
        /// The edges followed, in order.
        path: Vec<ReachHop>,
        /// The property read on the entities reached.
        read: Id,
        /// How several values became one answer.
        collapse: ReachCollapse,
        /// The entities actually reached.
        targets: Vec<Id>,
    },
    /// A constraint check.
    Constraint {
        /// Constraint identifier.
        constraint: Id,
        /// Whether the candidate passed.
        passed: bool,
    },
    /// A candidate eliminated from consideration.
    Rejected {
        /// Candidate identifier.
        candidate: Id,
        /// Why it was eliminated.
        reason: String,
    },
    /// A deterministic tie-break between equally scored candidates.
    TieBreak {
        /// Candidate that won.
        winner: Id,
        /// Rule applied to break the tie.
        rule: String,
    },
    /// The final selection.
    Decision {
        /// Candidate that was selected.
        candidate: Id,
    },
}

/// One step of a recorded traversal.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ReachHop {
    /// The relationship type followed.
    pub edge: Id,
    /// Whether it was followed forwards.
    pub outward: bool,
}

/// How a recorded traversal turned several values into one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ReachCollapse {
    /// Exactly one target.
    The,
    /// Held of every target.
    Every,
    /// Held of at least one.
    Some,
    /// Held of at least this many.
    AtLeast(u32),
}

/// One recorded reasoning step.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ProofNode {
    /// What this step was.
    pub kind: NodeKind,
    /// Nodes this step drew on, in engine order.
    pub inputs: Vec<NodeRef>,
}

/// An append-only record of a run.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct ProofGraph {
    nodes: Vec<ProofNode>,
}

impl ProofGraph {
    /// Start an empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a node and return its reference.
    pub fn push(&mut self, node: ProofNode) -> NodeRef {
        let index = self.nodes.len() as u32;
        self.nodes.push(node);
        NodeRef(index)
    }

    /// Borrow the recorded nodes in engine order.
    #[must_use]
    pub fn nodes(&self) -> &[ProofNode] {
        &self.nodes
    }

    /// Number of recorded nodes, reported by the benchmark suite as proof
    /// size.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The `ProofHash` for this graph.
    #[must_use]
    pub fn proof_hash(&self) -> Hash32 {
        self.canonical_hash()
    }
}

/// A graph under construction, with the provenance needed to wire it up.
///
/// Reasoners append through this rather than straight into a [`ProofGraph`],
/// so that a node which reads a property can be linked to whichever node
/// established that property — the evidence resolution, the declared input,
/// or the earlier rule that inferred it.
///
/// The two append methods differ only in whether the node becomes the new
/// source for a subject:
///
/// - [`record`](Self::record) — a step that observes or decides, but does
///   not itself establish a fact (a rule evaluation, a rejected candidate).
/// - [`establish`](Self::establish) — a step that puts a value into state,
///   and is therefore what later steps reading that subject depend on.
#[derive(Clone, Debug, Default)]
pub struct Trace {
    graph: ProofGraph,
    established: DetMap<Id, NodeRef>,
}

impl Trace {
    /// Start an empty trace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a node without claiming it established anything.
    pub fn record(&mut self, node: ProofNode) -> NodeRef {
        self.graph.push(node)
    }

    /// Append a node and make it the source of `subject`.
    ///
    /// A later establishment replaces an earlier one, because a property's
    /// provenance is whatever most recently set it. That matches the way
    /// the state merge rule works, and it means a value later overruled by
    /// a rule traces to the rule rather than to the observation it replaced.
    pub fn establish(&mut self, subject: Id, node: ProofNode) -> NodeRef {
        let reference = self.graph.push(node);
        self.established.insert(subject, reference);
        reference
    }

    /// Which node established `subject`, if any is recorded.
    #[must_use]
    pub fn source_of(&self, subject: &Id) -> Option<NodeRef> {
        self.established.get(subject).copied()
    }

    /// Resolve many subjects to the nodes that established them.
    ///
    /// Sorted and deduplicated. Two conditions reading the same property
    /// are one dependency, not two, and the edge list must not depend on
    /// the order the caller happened to collect subjects in — it is hashed.
    /// Subjects nothing established (a rule reading a property no step ever
    /// set) simply contribute no edge, which is the truthful encoding of
    /// "this rested on nothing recorded".
    #[must_use]
    pub fn sources_of<'a>(&self, subjects: impl IntoIterator<Item = &'a Id>) -> Vec<NodeRef> {
        let mut refs: Vec<NodeRef> = subjects
            .into_iter()
            .filter_map(|subject| self.source_of(subject))
            .collect();
        refs.sort_unstable();
        refs.dedup();
        refs
    }

    /// Borrow the graph built so far.
    #[must_use]
    pub fn graph(&self) -> &ProofGraph {
        &self.graph
    }

    /// Take the finished graph, dropping the build-time provenance map.
    #[must_use]
    pub fn into_graph(self) -> ProofGraph {
        self.graph
    }
}

impl Canonical for ProofNode {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("proof-node");
        write_kind(&self.kind, out);
        out.write(&self.inputs);
    }
}

impl Canonical for ProofGraph {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("proof-graph").write(&self.nodes);
    }
}

/// Encode a node kind with an explicit stable tag.
///
/// Tags are spelled out rather than derived from variant names so that a
/// rename cannot silently invalidate stored proof hashes.
fn write_kind(kind: &NodeKind, out: &mut CanonicalWriter) {
    match kind {
        NodeKind::Input { subject } => {
            out.write_str("input").write(subject);
        }
        NodeKind::RuleEvaluated { rule, fired } => {
            out.write_str("rule-evaluated")
                .write(rule)
                .write_bool(*fired);
        }
        NodeKind::Inference { subject } => {
            out.write_str("inference").write(subject);
        }
        NodeKind::Feature {
            feature,
            subject,
            resolved,
        } => {
            out.write_str("feature")
                .write(feature)
                .write(subject)
                .write_bool(*resolved);
        }
        NodeKind::Geometry { operator } => {
            out.write_str("geometry").write(operator);
        }
        NodeKind::Candidate { candidate } => {
            out.write_str("candidate").write(candidate);
        }
        NodeKind::Observed {
            observation,
            source,
        } => {
            out.write_str("observed").write(observation).write(source);
        }
        NodeKind::ObservationFailed {
            observation,
            source,
            reason,
        } => {
            out.write_str("observation-failed")
                .write(observation)
                .write(source)
                .write_str(reason);
        }
        NodeKind::Explanation {
            hypothesis,
            score,
            coverage,
            supported,
            contradicted,
            unexplained,
            assumptions,
        } => {
            out.write_str("explanation")
                .write(hypothesis)
                .write(score)
                .write(coverage)
                .write_u64(u64::from(*supported))
                .write_u64(u64::from(*contradicted))
                .write_u64(u64::from(*unexplained))
                .write_u64(u64::from(*assumptions));
        }
        NodeKind::Reach {
            anchor,
            path,
            read,
            collapse,
            targets,
        } => {
            out.write_str("reach").write(anchor);
            out.write_u64(path.len() as u64);
            for hop in path {
                out.write(&hop.edge).write_bool(hop.outward);
            }
            out.write(read);
            // Tags spelled out, per the discipline stated at the top of this
            // function: a rename must not silently invalidate stored hashes.
            match collapse {
                ReachCollapse::The => out.write_str("the"),
                ReachCollapse::Every => out.write_str("every"),
                ReachCollapse::Some => out.write_str("some"),
                ReachCollapse::AtLeast(count) => {
                    out.write_str("at-least").write_u64(u64::from(*count))
                }
            };
            out.write_u64(targets.len() as u64);
            for target in targets {
                out.write(target);
            }
        }
        NodeKind::Constraint { constraint, passed } => {
            out.write_str("constraint")
                .write(constraint)
                .write_bool(*passed);
        }
        NodeKind::Rejected { candidate, reason } => {
            out.write_str("rejected").write(candidate).write_str(reason);
        }
        NodeKind::TieBreak { winner, rule } => {
            out.write_str("tie-break").write(winner).write_str(rule);
        }
        NodeKind::Decision { candidate } => {
            out.write_str("decision").write(candidate);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn node(kind: NodeKind, inputs: Vec<NodeRef>) -> ProofNode {
        ProofNode { kind, inputs }
    }

    #[test]
    fn establishing_a_subject_makes_it_findable() {
        let mut trace = Trace::new();
        let input = trace.establish(
            id("machine.temperature"),
            node(
                NodeKind::Input {
                    subject: id("machine.temperature"),
                },
                Vec::new(),
            ),
        );

        assert_eq!(trace.source_of(&id("machine.temperature")), Some(input));
        assert_eq!(trace.source_of(&id("machine.vibration")), None);
    }

    #[test]
    fn recording_does_not_claim_provenance() {
        // A rule evaluation reads properties; it does not set any. If
        // `record` established something, a later rule would trace its
        // inputs to an evaluation rather than to the value it read.
        let mut trace = Trace::new();
        trace.record(node(
            NodeKind::RuleEvaluated {
                rule: id("R001"),
                fired: true,
            },
            Vec::new(),
        ));
        assert_eq!(trace.source_of(&id("R001")), None);
    }

    #[test]
    fn the_latest_establishment_wins() {
        // A property inferred over an observation traces to the inference.
        // Provenance is what set the value that is there now, not what set
        // it first.
        let subject = id("machine.bearing_fault");
        let mut trace = Trace::new();
        trace.establish(
            subject.clone(),
            node(
                NodeKind::Input {
                    subject: subject.clone(),
                },
                Vec::new(),
            ),
        );
        let later = trace.establish(
            subject.clone(),
            node(
                NodeKind::Inference {
                    subject: subject.clone(),
                },
                Vec::new(),
            ),
        );

        assert_eq!(trace.source_of(&subject), Some(later));
    }

    #[test]
    fn sources_are_sorted_and_deduplicated() {
        // The edge list is hashed, so it must not depend on the order the
        // caller happened to walk a rule's conditions in.
        let mut trace = Trace::new();
        let first = trace.establish(
            id("a.x"),
            node(NodeKind::Input { subject: id("a.x") }, Vec::new()),
        );
        let second = trace.establish(
            id("a.y"),
            node(NodeKind::Input { subject: id("a.y") }, Vec::new()),
        );

        let forward = trace.sources_of([&id("a.x"), &id("a.y"), &id("a.x")]);
        let backward = trace.sources_of([&id("a.y"), &id("a.x")]);

        assert_eq!(forward, vec![first, second]);
        assert_eq!(forward, backward);
    }

    #[test]
    fn an_unestablished_subject_contributes_no_edge() {
        // Silently inventing an edge would be worse than a missing one: it
        // would assert a dependency the run never had.
        let trace = Trace::new();
        assert!(trace.sources_of([&id("nothing.here")]).is_empty());
    }

    #[test]
    fn provenance_stays_out_of_the_hashed_graph() {
        // Two graphs with the same nodes must hash the same however they
        // were built, or the ProofHash would depend on scaffolding.
        let mut traced = Trace::new();
        traced.establish(
            id("a.x"),
            node(NodeKind::Input { subject: id("a.x") }, Vec::new()),
        );

        let mut plain = ProofGraph::new();
        plain.push(node(NodeKind::Input { subject: id("a.x") }, Vec::new()));

        assert_eq!(traced.graph().proof_hash(), plain.proof_hash());
        assert_eq!(traced.into_graph(), plain);
    }

    #[test]
    fn edges_reach_the_proof_hash() {
        // The complement of the test above: what a step rested on is part
        // of the reasoning, so two runs agreeing on every step but
        // disagreeing on the dependencies are not the same proof.
        let kind = NodeKind::Inference { subject: id("a.x") };

        let mut without = ProofGraph::new();
        without.push(node(NodeKind::Input { subject: id("a.x") }, Vec::new()));
        without.push(node(kind.clone(), Vec::new()));

        let mut with = ProofGraph::new();
        with.push(node(NodeKind::Input { subject: id("a.x") }, Vec::new()));
        with.push(node(kind, vec![NodeRef(0)]));

        assert_ne!(without.proof_hash(), with.proof_hash());
    }
}
