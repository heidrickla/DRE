//! Runtime configuration and resource limits.
//!
//! The ceilings here are the ones in plan section 22. They are engineering
//! limits rather than theoretical ones, and exist for two reasons.
//!
//! The first is termination: an unbounded planner search is not a slow
//! decision, it is no decision at all.
//!
//! The second is that limits are part of the engine identity. Two runs
//! under different ceilings can legitimately reach different answers —
//! one explored a state the other never reached. So the config is folded
//! into the `EngineHash` rather than treated as an ambient setting, which
//! makes that difference visible instead of looking like a determinism bug.

use serde::{Deserialize, Serialize};

use crate::canonical::{Canonical, CanonicalWriter};
use crate::error::{CoreError, Result};

/// Ceilings applied during a run.
///
/// Every field carries the MVP default, so an experiment that needs a
/// bigger budget may raise the one ceiling it needs rather than restating
/// all eight. Raising any of them changes the `EngineHash`, which is the
/// point — see the module note.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Maximum entities in canonical state.
    #[serde(default = "default_entities")]
    pub max_entities: u64,
    /// Maximum features across all entities.
    #[serde(default = "default_features")]
    pub max_features: u64,
    /// Maximum rules in a model pack.
    #[serde(default = "default_rules")]
    pub max_rules: u64,
    /// Maximum dimensions in a geometric space.
    #[serde(default = "default_dimensions")]
    pub max_dimensions: u64,
    /// Maximum regions in a geometric space.
    #[serde(default = "default_regions")]
    pub max_regions: u64,
    /// Maximum actions available to the planner.
    #[serde(default = "default_actions")]
    pub max_actions: u64,
    /// Maximum planner search depth.
    #[serde(default = "default_depth")]
    pub max_depth: u64,
    /// Maximum states the planner may expand.
    #[serde(default = "default_states")]
    pub max_states: u64,
}

const fn default_entities() -> u64 {
    Limits::MVP.max_entities
}
const fn default_features() -> u64 {
    Limits::MVP.max_features
}
const fn default_rules() -> u64 {
    Limits::MVP.max_rules
}
const fn default_dimensions() -> u64 {
    Limits::MVP.max_dimensions
}
const fn default_regions() -> u64 {
    Limits::MVP.max_regions
}
const fn default_actions() -> u64 {
    Limits::MVP.max_actions
}
const fn default_depth() -> u64 {
    Limits::MVP.max_depth
}
const fn default_states() -> u64 {
    Limits::MVP.max_states
}

impl Limits {
    /// The MVP defaults from plan section 22.
    pub const MVP: Self = Self {
        max_entities: 10_000,
        max_features: 10_000,
        max_rules: 10_000,
        max_dimensions: 256,
        max_regions: 1_000,
        max_actions: 1_000,
        max_depth: 20,
        max_states: 100_000,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::MVP
    }
}

impl Canonical for Limits {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("limits")
            .write_u64(self.max_entities)
            .write_u64(self.max_features)
            .write_u64(self.max_rules)
            .write_u64(self.max_dimensions)
            .write_u64(self.max_regions)
            .write_u64(self.max_actions)
            .write_u64(self.max_depth)
            .write_u64(self.max_states);
    }
}

/// Whether a run records a full proof.
///
/// Proof recording is a memory cost, not a correctness one, so it can be
/// dialled down for large benchmark sweeps. It is part of the engine
/// identity because a run without a proof cannot produce a `ProofHash`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofMode {
    /// Record every step, including rejected candidates. The default,
    /// because "explain exactly how that result was reached" is one of the
    /// MVP's stated objectives rather than a debugging affordance.
    #[default]
    Full,
    /// Record only the steps that contributed to the final decision.
    Contributing,
    /// Record nothing. No `ProofHash` is produced.
    Off,
}

/// How the planner searches.
///
/// Engine configuration rather than planner-internal detail, which is why
/// it lives here beside the ceilings and the proof mode. It reaches the
/// planner through [`RuntimeConfig`] and is therefore in the `EngineHash`.
///
/// That placement is a finding rather than a default. `astar_matches_dijkstra`
/// asks whether A\* and Dijkstra return the *same* optimal plan, and treats
/// a disagreement as the trigger for putting `Strategy` in the engine
/// identity. Measured on a 40-by-40 grid with a mid-grid goal, they agree:
/// same plan, 383 states expanded against 727. But agreeing on the answer
/// is only half of what the five hashes verify — expanding half as many
/// states means recording a different set of rejected candidates, so the
/// `ProofHash` differs while the `DecisionHash` does not.
///
/// A strategy left out of the engine identity would therefore make two
/// legitimately different searches report `ProofMismatch` against one
/// another with every other hash equal, which reads as a determinism
/// failure and is not one. It belongs here for the proof's sake, not the
/// decision's.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// Breadth-first. Ignores cost; finds the shallowest plan.
    #[default]
    BreadthFirst,
    /// Uniform-cost search.
    Dijkstra,
    /// Dijkstra with a goal-distance heuristic, typically the weighted
    /// squared distance from `dre-geometry`.
    #[serde(rename = "astar")]
    AStar,
}

impl Strategy {
    /// The canonical spelling, and the one an experiment file writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BreadthFirst => "breadth_first",
            Self::Dijkstra => "dijkstra",
            Self::AStar => "astar",
        }
    }
}

/// Complete runtime configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Resource ceilings.
    pub limits: Limits,
    /// Proof recording level.
    pub proof_mode: ProofMode,

    /// How the planner searches.
    ///
    /// Defaults to [`Strategy::Dijkstra`] rather than to
    /// `Strategy::default()`, which is breadth-first. The difference is
    /// deliberate: the lab runner searched with Dijkstra from the first
    /// experiment to the last, so this default is what every stored record
    /// was actually produced under. Taking the enum's own default here
    /// would silently re-plan every existing experiment by a different
    /// search — a reasoning change smuggled in as a tidiness one.
    pub strategy: Strategy,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            limits: Limits::MVP,
            proof_mode: ProofMode::Full,
            strategy: Strategy::Dijkstra,
        }
    }
}

impl RuntimeConfig {
    /// Validate internal consistency.
    ///
    /// A zero ceiling would make every run fail at the first step, which is
    /// better caught at load than mid-decision.
    pub fn validate(&self) -> Result<()> {
        let checks: [(&'static str, u64); 8] = [
            ("max_entities", self.limits.max_entities),
            ("max_features", self.limits.max_features),
            ("max_rules", self.limits.max_rules),
            ("max_dimensions", self.limits.max_dimensions),
            ("max_regions", self.limits.max_regions),
            ("max_actions", self.limits.max_actions),
            ("max_depth", self.limits.max_depth),
            ("max_states", self.limits.max_states),
        ];

        for (name, value) in checks {
            if value == 0 {
                return Err(CoreError::LimitExceeded {
                    limit: name,
                    cap: 0,
                });
            }
        }

        Ok(())
    }
}

impl Canonical for RuntimeConfig {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("runtime-config").write(&self.limits);
        let mode = match self.proof_mode {
            ProofMode::Full => "full",
            ProofMode::Contributing => "contributing",
            ProofMode::Off => "off",
        };
        out.write_str(mode);
        out.write_str(self.strategy.as_str());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn default_config_is_valid() {
        RuntimeConfig::default()
            .validate()
            .expect("MVP defaults valid");
    }

    #[test]
    fn rejects_zero_ceilings() {
        let mut config = RuntimeConfig::default();
        config.limits.max_depth = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_difference_changes_engine_identity() {
        let baseline = RuntimeConfig::default();
        let mut deeper = baseline;
        deeper.limits.max_depth = 40;
        // Different ceilings must not silently look like the same engine.
        assert_ne!(baseline.canonical_hash(), deeper.canonical_hash());
    }

    #[test]
    fn proof_mode_changes_engine_identity() {
        let baseline = RuntimeConfig::default();
        let quiet = RuntimeConfig {
            proof_mode: ProofMode::Off,
            ..baseline
        };
        assert_ne!(baseline.canonical_hash(), quiet.canonical_hash());
    }

    #[test]
    fn strategy_changes_engine_identity() {
        // Not because the two disagree about the answer -- measured, they
        // do not -- but because they expand different states and therefore
        // record different proofs. Two searches whose `ProofHash` differs
        // must not share an `EngineHash`, or the difference reports as a
        // determinism failure instead of as a different question.
        let baseline = RuntimeConfig::default();
        let guided = RuntimeConfig {
            strategy: Strategy::AStar,
            ..baseline
        };
        assert_ne!(baseline.canonical_hash(), guided.canonical_hash());
    }

    #[test]
    fn the_default_strategy_is_what_the_records_were_made_under() {
        // The runner hardcoded Dijkstra for every stored record. If this
        // ever becomes `Strategy::default()` -- breadth-first -- every
        // existing experiment is silently re-planned by a different search.
        assert_eq!(RuntimeConfig::default().strategy, Strategy::Dijkstra);
        assert_ne!(Strategy::default(), Strategy::Dijkstra);
    }
}
