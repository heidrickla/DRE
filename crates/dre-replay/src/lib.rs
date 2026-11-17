//! Phase 11 — Replay.
//!
//! Every experiment produces the five hashes from plan section 17, and
//! replay verifies that re-running reproduces them.
//!
//! Splitting the record into five hashes rather than one is what makes a
//! failure diagnosable. A single combined digest tells you that something
//! changed; these tell you *what*. A differing `EngineHash` with matching
//! inputs means the engine was rebuilt with different configuration. A
//! matching `DecisionHash` with a differing `ProofHash` means the engine
//! reached the same answer by a different route — still a determinism
//! failure, and one that a decision-only comparison would miss entirely.
//!
//! Status: skeleton. The record and verification are implemented; storage
//! and the experiment runner are not.

pub mod store;

pub use store::{StoreError, StoredRecord};

use dre_core::canonical::{Canonical, CanonicalWriter};
use dre_core::hash::Hash32;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};

/// The hashes identifying one experiment run.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ReplayRecord {
    /// Experiment identifier.
    pub experiment: Id,
    /// Hash of the canonical input state.
    pub input: Hash32,
    /// Hash of the model pack.
    pub model: Hash32,
    /// Hash of the engine version and configuration.
    pub engine: Hash32,
    /// Hash of the decision.
    pub decision: Hash32,
    /// Hash of the proof graph.
    pub proof: Hash32,
}

impl Canonical for ReplayRecord {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("replay-record")
            .write(&self.experiment)
            .write(&self.input)
            .write(&self.model)
            .write(&self.engine)
            .write(&self.decision)
            .write(&self.proof);
    }
}

/// Which of the three preconditions for comparison failed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precondition {
    /// The canonical input differed.
    Input,
    /// The model pack differed.
    Model,
    /// The engine version or configuration differed.
    Engine,
}

/// The outcome of verifying a replay.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Decision and proof both reproduced exactly.
    Match,
    /// The runs are not comparable, so nothing is proven either way.
    NotComparable {
        /// The preconditions that differed, in a fixed order.
        differing: Vec<Precondition>,
    },
    /// Same inputs, different decision. A determinism failure.
    DecisionMismatch {
        /// The originally recorded decision hash.
        expected: Hash32,
        /// The hash produced by the replay.
        actual: Hash32,
    },
    /// Same inputs and same decision, different reasoning. Also a
    /// determinism failure, and the one most likely to go unnoticed.
    ProofMismatch {
        /// The originally recorded proof hash.
        expected: Hash32,
        /// The hash produced by the replay.
        actual: Hash32,
    },
}

impl Verdict {
    /// Whether the replay reproduced the original exactly.
    #[must_use]
    pub fn is_match(&self) -> bool {
        matches!(self, Self::Match)
    }
}

/// Verify that a replay reproduced an original run.
///
/// Preconditions are checked before outcomes: comparing decisions across
/// runs that had different inputs would report a mismatch that says
/// nothing about determinism.
#[must_use]
pub fn verify(original: &ReplayRecord, replayed: &ReplayRecord) -> Verdict {
    let mut differing = Vec::new();
    if original.input != replayed.input {
        differing.push(Precondition::Input);
    }
    if original.model != replayed.model {
        differing.push(Precondition::Model);
    }
    if original.engine != replayed.engine {
        differing.push(Precondition::Engine);
    }

    if !differing.is_empty() {
        return Verdict::NotComparable { differing };
    }

    if original.decision != replayed.decision {
        return Verdict::DecisionMismatch {
            expected: original.decision,
            actual: replayed.decision,
        };
    }

    if original.proof != replayed.proof {
        return Verdict::ProofMismatch {
            expected: original.proof,
            actual: replayed.proof,
        };
    }

    Verdict::Match
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn record(decision: &[u8], proof: &[u8]) -> ReplayRecord {
        ReplayRecord {
            experiment: Id::new("EXP-000421").expect("valid id"),
            input: Hash32::of(b"input"),
            model: Hash32::of(b"model"),
            engine: Hash32::of(b"engine"),
            decision: Hash32::of(decision),
            proof: Hash32::of(proof),
        }
    }

    #[test]
    fn identical_runs_match() {
        let original = record(b"A004", b"proof");
        assert_eq!(verify(&original, &original.clone()), Verdict::Match);
    }

    #[test]
    fn differing_decision_is_a_mismatch() {
        let original = record(b"A004", b"proof");
        let replayed = record(b"A007", b"proof");
        assert!(matches!(
            verify(&original, &replayed),
            Verdict::DecisionMismatch { .. }
        ));
    }

    #[test]
    fn same_decision_by_different_route_still_fails() {
        let original = record(b"A004", b"proof-via-rules");
        let replayed = record(b"A004", b"proof-via-geometry");
        // The case a decision-only comparison would silently pass.
        assert!(matches!(
            verify(&original, &replayed),
            Verdict::ProofMismatch { .. }
        ));
    }

    #[test]
    fn differing_preconditions_prove_nothing() {
        let original = record(b"A004", b"proof");
        let mut replayed = original.clone();
        replayed.model = Hash32::of(b"model-v2");
        replayed.decision = Hash32::of(b"A007");

        assert_eq!(
            verify(&original, &replayed),
            Verdict::NotComparable {
                differing: vec![Precondition::Model]
            }
        );
    }
}
