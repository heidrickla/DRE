//! Human-readable proof rendering.
//!
//! The proof graph is the engine's answer to "how did you reach that",
//! and a structure nobody can read does not answer it. This renders the
//! shape shown in plan section 16.
//!
//! Rendering is display only. Nothing here feeds back into a decision, and
//! the `ProofHash` covers the graph rather than this text — so the format
//! can change without invalidating stored records.

use std::fmt::Write as _;

use dre_core::id::Id;

use crate::{NodeKind, NodeRef, ProofGraph, ReachCollapse};

/// Render a proof as an indented trace.
///
/// Each line carries its own index and the indices it drew on, because a
/// proof whose dependency edges are invisible reads as a flat list of
/// events — the ordering suggests causation that the edges actually
/// record. `#7 <- #2 #4` says step 7 rested on steps 2 and 4, and nothing
/// else.
#[must_use]
pub fn render(proof: &ProofGraph) -> String {
    let mut out = String::new();

    if proof.is_empty() {
        return "(no proof recorded)\n".to_owned();
    }

    for (index, node) in proof.nodes().iter().enumerate() {
        if index > 0 {
            out.push_str("   │\n   ▼\n");
        }
        let _ = writeln!(
            out,
            "#{index} {}{}",
            describe(&node.kind),
            show_inputs(&node.inputs)
        );
    }

    out
}

/// One line describing what a node recorded.
fn describe(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Input { subject } => format!("INPUT      {subject}"),
        NodeKind::RuleEvaluated { rule, fired } => {
            let verdict = if *fired { "fired" } else { "did not fire" };
            format!("RULE       {rule} ({verdict})")
        }
        NodeKind::Inference { subject } => format!("INFER      {subject}"),
        NodeKind::Feature {
            feature,
            subject,
            resolved,
        } => {
            let verdict = if *resolved {
                "derived"
            } else {
                "COULD NOT DERIVE"
            };
            format!("FEATURE    {feature} -> {subject} ({verdict})")
        }
        NodeKind::Geometry { operator } => format!("GEOMETRY   {operator}"),
        NodeKind::Candidate { candidate } => format!("CANDIDATE  {candidate}"),
        NodeKind::ObservationFailed {
            observation,
            source,
            reason,
        } => format!("NO READING {observation} from {source} — {reason}"),
        NodeKind::Observed {
            observation,
            source,
        } => format!("OBSERVED   {observation} from {source}"),
        NodeKind::Explanation {
            hypothesis,
            score,
            coverage,
            supported,
            contradicted,
            unexplained,
            assumptions,
        } => format!(
            "EXPLAINS   {hypothesis} scored {score} \
             (coverage {coverage}, {supported} supported, {contradicted} contradicted, \
             {unexplained} unexplained, {assumptions} assumed)"
        ),
        NodeKind::Reach {
            anchor,
            path,
            read,
            collapse,
            targets,
        } => {
            let route = path
                .iter()
                .map(|hop| {
                    let arrow = if hop.outward { "->" } else { "<-" };
                    format!("{arrow}{}", hop.edge)
                })
                .collect::<Vec<_>>()
                .join(" ");
            let reached = if targets.is_empty() {
                "nothing".to_owned()
            } else {
                targets
                    .iter()
                    .map(Id::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let word = match collapse {
                ReachCollapse::The => "the".to_owned(),
                ReachCollapse::Every => "every".to_owned(),
                ReachCollapse::Some => "some".to_owned(),
                ReachCollapse::AtLeast(count) => format!("at least {count}"),
            };
            format!("REACH      {anchor} {route} .{read} ({word} of {reached})")
        }
        NodeKind::Constraint { constraint, passed } => {
            let verdict = if *passed { "passed" } else { "FAILED" };
            format!("CONSTRAINT {constraint} ({verdict})")
        }
        NodeKind::Rejected { candidate, reason } => {
            format!("REJECTED   {candidate} — {reason}")
        }
        NodeKind::TieBreak { winner, rule } => format!("TIE-BREAK  {winner} by {rule}"),
        NodeKind::Decision { candidate } => format!("DECISION   {candidate}"),
    }
}

/// Render a node's dependency edges, or nothing when it has none.
fn show_inputs(inputs: &[NodeRef]) -> String {
    if inputs.is_empty() {
        return String::new();
    }

    let mut out = String::from("  <-");
    for NodeRef(index) in inputs {
        let _ = write!(out, " #{index}");
    }
    out
}

/// Render only the steps a given node transitively rested on.
///
/// The question this answers is the one a disputed result actually
/// prompts: not "what did the engine do" but "what did *this* conclusion
/// depend on". On a run with a few hundred nodes the full trace buries the
/// handful that mattered.
///
/// Nodes come back in engine order, so the chain reads forward from the
/// earliest input to the conclusion. Returns `None` when the index is not
/// in the graph.
#[must_use]
pub fn explain(proof: &ProofGraph, node: NodeRef) -> Option<String> {
    let NodeRef(index) = node;
    let total = u32::try_from(proof.len()).unwrap_or(u32::MAX);
    if index >= total {
        return None;
    }

    // Reachability by reverse traversal. Edges always point backwards --
    // a node can only depend on one already appended -- so a descending
    // sweep needs no work list and cannot loop.
    let mut wanted = vec![false; proof.len()];
    if let Some(slot) = wanted.get_mut(index as usize) {
        *slot = true;
    }

    for (position, entry) in proof.nodes().iter().enumerate().rev() {
        if !wanted.get(position).copied().unwrap_or(false) {
            continue;
        }
        for NodeRef(input) in &entry.inputs {
            if let Some(slot) = wanted.get_mut(*input as usize) {
                *slot = true;
            }
        }
    }

    let mut out = String::new();
    for (position, entry) in proof.nodes().iter().enumerate() {
        if !wanted.get(position).copied().unwrap_or(false) {
            continue;
        }
        let _ = writeln!(
            out,
            "#{position} {}{}",
            describe(&entry.kind),
            show_inputs(&entry.inputs)
        );
    }

    Some(out)
}

/// A one-line summary of what the proof contains.
#[must_use]
pub fn summarize(proof: &ProofGraph) -> String {
    let mut evaluated = 0usize;
    let mut fired = 0usize;
    let mut inferences = 0usize;
    let mut rejected = 0usize;

    for node in proof.nodes() {
        match &node.kind {
            NodeKind::RuleEvaluated { fired: did, .. } => {
                evaluated += 1;
                if *did {
                    fired += 1;
                }
            }
            NodeKind::Inference { .. } => inferences += 1,
            NodeKind::Rejected { .. } => rejected += 1,
            _ => {}
        }
    }

    format!(
        "{} nodes: {evaluated} rules evaluated, {fired} fired, {inferences} inferences, {rejected} rejected",
        proof.len()
    )
}
