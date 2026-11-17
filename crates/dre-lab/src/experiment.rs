//! Experiment definitions.
//!
//! An experiment pairs a model pack with an input state and the result
//! expected from it. Running one produces the five hashes of plan section
//! 17, which is what makes the run replayable rather than merely repeated.
//!
//! The input is declared here rather than inside the pack on purpose: the
//! pack is the *domain*, the experiment is a *question asked of it*. Mixing
//! them would mean a new question needed a new pack, and the MVP's domain
//! isolation claim would be untestable.

use std::path::{Path, PathBuf};

use dre_core::id::Id;
use dre_state::{EpistemicStatus, Value};
use serde::Deserialize;

/// Every experiment file in a directory, in identifier order.
///
/// Tests that mean "every experiment" must enumerate rather than list.
/// Both `mvp_experiments.rs` and `stored_replay.rs` carried a hand-written
/// array of twelve names, which is a claim about coverage that goes stale
/// silently: adding a thirteenth experiment left it asserted by nothing at
/// all, and the suite stayed green because the array was still internally
/// consistent. That is the same shape as the defect those tests were
/// written to catch — an experiment nobody checks is a comment — reappearing
/// one level up, in the list of what gets checked.
///
/// Sorted, because `read_dir` order is filesystem-dependent and a test that
/// iterates it would report failures in an order that varied by machine.
///
/// # Errors
///
/// Propagates any failure to read `dir`.
pub fn files_in(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    found.sort();
    Ok(found)
}

/// A complete experiment definition, the shape of an `experiments/*.yaml`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentFile {
    /// The experiment.
    pub experiment: Experiment,
}

/// One experiment.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique identifier, used to name replay records.
    pub id: Id,
    /// Human-facing description.
    #[serde(default)]
    pub description: String,
    /// Path to the model pack, relative to the repository root.
    pub pack: PathBuf,

    /// Ceilings for this run, overriding the MVP defaults.
    ///
    /// Absent means the defaults, which is why adding this field left every
    /// existing experiment's `EngineHash` untouched.
    ///
    /// Declaring limits here is not a way to make a problem fit. It is an
    /// admission that the problem needs a different engine, and the
    /// `EngineHash` says so: a run under raised ceilings can legitimately
    /// reach a different answer, because it explored states the default
    /// engine would have refused. Replay against a record made under other
    /// limits reports NOT COMPARABLE rather than a mismatch, which is the
    /// correct reading — the two runs were not asked the same question.
    #[serde(default)]
    pub limits: Option<dre_core::config::Limits>,

    /// How the planner searches, overriding the default of `dijkstra`.
    ///
    /// Absent means the default, which is what every record made before
    /// this field existed was produced under.
    ///
    /// This is engine configuration and lands in the `EngineHash`, so two
    /// experiments differing only here are not the same question. That is
    /// the correct reading even though the answers agree: A\* and Dijkstra
    /// return the same optimal plan, but A\* reaches it having rejected
    /// fewer candidates, and the proof records what was rejected.
    #[serde(default)]
    pub strategy: Option<dre_core::config::Strategy>,

    /// The starting state.
    pub given: Given,
    /// What the result must be for the experiment to pass.
    #[serde(default)]
    pub expect: Expect,
}

/// One declared edge between two entities.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipDecl {
    /// Unique identifier for this edge.
    pub id: Id,
    /// The relationship type it instantiates.
    #[serde(rename = "type")]
    pub type_id: Id,
    /// Entity the edge runs from.
    pub from: Id,
    /// Entity the edge runs to.
    pub to: Id,
}

/// The declared starting state.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Given {
    /// Entities and their initial properties.
    #[serde(default)]
    pub entities: Vec<EntityDecl>,

    /// Edges between the declared entities.
    ///
    /// Topology is **input**, not model, for the same reason evidence is: the
    /// pack describes what kinds of thing exist and how they may relate, and
    /// the experiment says which ones actually do. Two different sites with
    /// the same equipment are two questions asked of one domain.
    ///
    /// `bus-independence` is the standing argument for having this at all —
    /// the engine cannot reason around topology that was never declared, and
    /// `bus-independence-003` shows it acting confidently and wrongly on
    /// correlation nobody wrote down.
    #[serde(default)]
    pub relationships: Vec<RelationshipDecl>,

    /// Sources that evidence may be attributed to.
    #[serde(default)]
    pub sources: Vec<dre_evidence::Source>,

    /// Observations attributed to sources.
    ///
    /// Evidence is *input*, not model — which is why it lives here rather
    /// than in a pack section. The pack describes a domain; the experiment
    /// is a question asked of it, and what the sensors happened to report
    /// is part of the question.
    #[serde(default)]
    pub evidence: Vec<dre_evidence::Evidence>,

    /// The logical tick the question is asked at, for freshness.
    ///
    /// Never a clock reading. The same experiment run today and next year
    /// must reach the same conclusion about what is stale.
    #[serde(default)]
    pub now: u64,

    /// Observations that were attempted and produced no value.
    ///
    /// A separate list from `evidence`, and deliberately so. A failed read
    /// is not a reading — it is a record that somebody looked and nothing
    /// came back — and keeping it out of `evidence` makes its exclusion
    /// from resolution **structural** rather than a filter that has to be
    /// remembered. This repository's recurring defect is things that should
    /// have been excluded and were not.
    ///
    /// It exists because an independent author asked for it: they wanted to
    /// state that "the PSU telemetry read itself failed" rather than merely
    /// omit the value, and their workaround lost the distinction between
    /// "no observation yet" and "the instrument attempted a measurement and
    /// failed".
    ///
    /// It records, and nothing more. No status, no operator, no way for a
    /// rule to branch on it — that would be a stronger requirement than the
    /// one actually found, and the same author declined to claim it.
    #[serde(default)]
    pub failed_reads: Vec<FailedRead>,

    /// Thresholds governing resolution.
    ///
    /// Declared per experiment rather than per pack so one domain can be
    /// tested under different policies — which is what a lab is for. If a
    /// domain ever needs a policy it *cannot* be run without, that is a
    /// signal it belongs in the pack instead.
    #[serde(default)]
    pub policy: dre_evidence::EvidencePolicy,
}

/// An observation that was attempted and produced nothing.
///
/// Carries why. A property left simply absent says "nobody looked"; this
/// says "somebody looked and the instrument did not answer", and for
/// instrumentation-heavy diagnosis those are different facts about the
/// world.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailedRead {
    /// Identifier, so the attempt can be named in the proof.
    pub id: Id,
    /// Entity the attempt concerned.
    pub subject: Id,
    /// Property that was being read.
    pub predicate: Id,
    /// Instrument or system that failed to produce a value.
    pub source: Id,
    /// Why no value came back. Required: a failed read with no reason is
    /// indistinguishable from an absent one, which is the distinction this
    /// exists to preserve.
    pub reason: String,
    /// Logical tick the attempt was made at.
    #[serde(default)]
    pub observed_at: u64,
}

/// One declared entity.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDecl {
    /// Entity identifier.
    pub id: Id,
    /// Entity type from the pack's ontology.
    #[serde(rename = "type")]
    pub type_id: Id,
    /// Initial properties, keyed by property name.
    #[serde(default)]
    pub properties: Vec<PropertyDecl>,
}

/// One declared property.
///
/// A property with a value is an observation; one without is a positive
/// claim about not knowing — `UNKNOWN`, `UNOBSERVABLE` or
/// `NOT_APPLICABLE` — which the engine treats as different situations.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyDecl {
    /// Property name.
    pub name: Id,
    /// Observed value, if any.
    #[serde(default)]
    pub value: Option<Value>,
    /// Epistemic status. Defaults to `KNOWN` when a value is present and
    /// `UNKNOWN` when it is not.
    #[serde(default)]
    pub status: Option<EpistemicStatus>,
}

impl PropertyDecl {
    /// The status this declaration implies.
    #[must_use]
    pub fn effective_status(&self) -> EpistemicStatus {
        self.status.unwrap_or(if self.value.is_some() {
            EpistemicStatus::Known
        } else {
            EpistemicStatus::Unknown
        })
    }
}

/// Assertions checked after the run.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Property outcomes, keyed by `entity.property`.
    #[serde(default)]
    pub properties: Vec<PropertyExpectation>,
    /// Exact number of rules that must fire.
    #[serde(default)]
    pub rules_fired: Option<u64>,
    /// Exact number of planner states that must be expanded.
    #[serde(default)]
    pub states_expanded: Option<u64>,
    /// The plan the planner must produce, as action identifiers in order.
    #[serde(default)]
    pub plan: Option<Vec<Id>>,

    /// How many steps the plan must have.
    ///
    /// The assertion that survives scale. Naming the route exactly is the
    /// stronger claim and the right one on a small pack, but a
    /// seventy-eight step plan transcribed into YAML is unreadable, and an
    /// unreadable assertion is one nobody checks when it fails.
    ///
    /// Length is also the more interesting property on a grid where
    /// thousands of routes are equally optimal: *which* one comes back is
    /// the frontier ordering's business and is pinned by the replay record,
    /// while the length is what says the answer is optimal at all.
    #[serde(default)]
    pub plan_length: Option<usize>,
}

/// One expected property outcome.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyExpectation {
    /// `entity.property`.
    pub subject: Id,
    /// Expected value, if asserted.
    #[serde(default)]
    pub value: Option<Value>,
    /// Expected status, if asserted.
    #[serde(default)]
    pub status: Option<EpistemicStatus>,
}
