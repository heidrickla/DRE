//! Phase 5 — Evidence reasoning.
//!
//! Lets the engine reason when information is incomplete or contradictory.
//!
//! The MVP deliberately avoids probabilistic inference. Bayesian updating
//! would require priors that no model pack can honestly supply yet, and it
//! would make every result a distribution needing a threshold somewhere
//! downstream anyway. Instead this uses deterministic thresholds and
//! precedence rules — cruder, but auditable: the proof can state exactly
//! which sources agreed and which rule promoted the result.
//!
//! Two conventions worth noting.
//!
//! Reliability is an integer in `0..=1000`, not a probability. It is a
//! precedence weight, and calling it a probability would invite arithmetic
//! that the semantics do not support.
//!
//! Freshness is measured in logical ticks supplied with the input, never
//! by reading a clock. An engine that consulted the wall clock would give
//! different answers to the same question asked twice, which is exactly
//! what this project exists to rule out.
//!
//! Resolution lives in [`resolve`].

pub mod resolve;

pub use resolve::{resolve_all, Resolved};

use dre_core::id::Id;
use dre_state::{EpistemicStatus, Value};
use serde::{Deserialize, Serialize};

/// Maximum source reliability weight.
pub const MAX_RELIABILITY: u32 = 1000;

/// A single observation attributed to a source.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Unique identifier.
    pub id: Id,
    /// Entity the claim is about.
    pub subject: Id,
    /// Property being claimed.
    pub predicate: Id,
    /// Claimed value.
    pub value: Value,
    /// Where the claim came from.
    pub source: Id,
    /// Source precedence weight, `0..=MAX_RELIABILITY`.
    pub reliability: u32,
    /// Logical tick at which the observation was made.
    pub observed_at: u64,
}

/// A source of evidence.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Unique identifier.
    pub id: Id,
    /// Default reliability for claims from this source.
    pub reliability: u32,
    /// Independence group. Two sources sharing a group are not treated as
    /// independent corroboration — otherwise three sensors on one failed
    /// bus would look like strong agreement rather than one common cause.
    pub independence_group: Option<Id>,
}

/// Outcome of resolving competing evidence for one property.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    /// Resolved value, absent when unresolved or conflicting.
    pub value: Option<Value>,
    /// Resulting epistemic status.
    pub status: EpistemicStatus,
    /// Evidence that supported the outcome.
    pub supporting: Vec<Id>,
    /// Evidence that contradicted it. Retained rather than discarded so
    /// the proof can show what was overruled and why.
    pub dissenting: Vec<Id>,
}

/// Deterministic thresholds governing resolution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePolicy {
    /// Reliability at or above which a source counts as high-reliability.
    pub high_reliability: u32,
    /// Independent high-reliability sources that must agree before a value
    /// is promoted to `KNOWN`.
    pub corroboration_required: u32,
    /// Age in logical ticks beyond which an observation becomes `STALE`.
    pub freshness_window: u64,
}

impl Default for EvidencePolicy {
    fn default() -> Self {
        // Matches the worked example in plan section 11: two independent
        // high-reliability sources agreeing promotes the value to KNOWN.
        Self {
            high_reliability: 800,
            corroboration_required: 2,
            freshness_window: 100,
        }
    }
}
