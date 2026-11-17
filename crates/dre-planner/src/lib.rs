//! Phases 8 and 9 — Constraint engine, goal model, and planner.
//!
//! Searches action sequences from the current state toward a goal, subject
//! to constraints.
//!
//! Determinism here is more delicate than elsewhere. Textbook A* and
//! Dijkstra leave node expansion order unspecified among equal-cost
//! frontier entries, and standard priority queue implementations resolve
//! that by heap layout — which depends on insertion history. Two runs that
//! inserted the same nodes in a different order can then expand a
//! different node first and return a different, equally optimal path. The
//! fix is that frontier ordering is total: cost, then depth, then action
//! identifier, which is unique.
//!
//! Search is always bounded (plan section 14). Exceeding a bound is an
//! error, never a truncated answer — see `ExecutionContext::charge_state_expansion`.
//!
//! # Scope note
//!
//! Constraints and goals live here rather than in a crate of their own.
//! The architecture diagram shows the constraint engine as its own stage,
//! but plan section 6 lists ten crates without one, and the planner is
//! their only consumer. If constraint evaluation grows independently
//! useful it should be split out.
//!
//! Breadth-first, Dijkstra and A\* are all implemented, and A\* refuses a
//! model whose heuristic it cannot bound rather than returning a plan it
//! cannot vouch for.

pub mod search;

pub use search::{
    check_admissible, plan, satisfied, ConstraintsSection, GoalsSection, SearchError,
};

use std::cmp::Ordering;

use dre_core::canonical::{Canonical, CanonicalWriter};
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_symbolic::Condition;
use serde::{Deserialize, Serialize};

/// Search strategy.
///
/// Defined in `dre-core` beside the ceilings and the proof mode, because it
/// is part of the engine identity rather than an argument the caller picks
/// freely — see [`dre_core::config::Strategy`]. Re-exported here because
/// this is the crate that acts on it.
pub use dre_core::config::Strategy;

/// A target state.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Goal {
    /// Unique identifier.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// Conditions that define the goal as reached.
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

/// A condition every candidate must satisfy.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    /// Unique identifier, recorded in the proof on pass and on fail.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// Conditions that must hold.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Whether violating this constraint eliminates a candidate outright.
    ///
    /// A soft constraint penalises instead, which lets a model express
    /// "avoid the hazard unless there is no other route" without making
    /// the problem unsolvable.
    #[serde(default)]
    pub hard: bool,

    /// Cost added when a soft constraint is violated. Ignored when `hard`.
    ///
    /// This is what makes "avoid unless you must" expressible: a hazard
    /// with a penalty of 5 is worth up to five extra steps of detour, and
    /// no more. A hard constraint cannot say that -- it can only say the
    /// route is forbidden however long the alternative.
    #[serde(default = "default_penalty")]
    pub penalty: Fixed,

    /// What this constraint does when its conditions cannot be decided.
    /// Ignored when the constraint is soft.
    #[serde(default)]
    pub on_undecidable: Undecidable,
}

/// What a hard constraint does when it cannot tell whether it is satisfied.
///
/// Both readings are correct, for different constraints, which is why this
/// is declared per constraint rather than chosen once for the engine:
///
/// - A **safety invariant** must block. "I cannot tell whether this is
///   safe" is not permission, and treating it as permission means the one
///   situation the constraint exists for -- missing information -- is the
///   one situation it does not cover.
/// - A **procedural or preference invariant** must permit, or nothing
///   plans at all: early in a search most properties are undecided, and a
///   blocking constraint prunes every candidate before the facts that
///   would satisfy it can be established.
///
/// It lives in the pack rather than in engine configuration on purpose. A
/// pack is hashed into `ModelHash`, so the choice travels with the model
/// that depends on it. An engine-level switch would put a pack's meaning
/// in a setting the pack cannot see -- which is the exact shape of every
/// defect this project has found so far.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Undecidable {
    /// Undecidable eliminates the candidate. The default.
    ///
    /// Default because the alternative derives a permission from an
    /// absence, and this engine's stated position is that nothing is
    /// derived from a default. A pack that needs the other reading says
    /// so, and the saying is recorded in its hash.
    #[default]
    Block,
    /// Undecidable leaves the candidate alone.
    Permit,
}

fn default_penalty() -> Fixed {
    Fixed::ONE
}

/// A frontier entry, ordered totally so expansion order is reproducible.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrontierEntry {
    /// Accumulated path cost, plus heuristic under A*.
    pub priority: Fixed,
    /// Depth from the initial state.
    pub depth: u64,
    /// Action that produced this node. Unique, so ordering never falls
    /// through to an arbitrary comparison.
    pub action: Id,
}

impl Ord for FrontierEntry {
    /// Cost ASC, then depth ASC, then action identifier ASC.
    ///
    /// Depth precedes identifier so that equal-cost plans prefer the
    /// shorter one, which is the answer a reader expects when two routes
    /// cost the same.
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.depth.cmp(&other.depth))
            .then_with(|| self.action.cmp(&other.action))
    }
}

impl PartialOrd for FrontierEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A resulting action sequence.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    /// Actions in execution order.
    pub actions: Vec<Id>,
    /// Total cost.
    pub cost: Fixed,
}

impl Canonical for Plan {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("plan").write(&self.actions).write(&self.cost);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn entry(priority: i64, depth: u64, action: &str) -> FrontierEntry {
        FrontierEntry {
            priority: Fixed::from_int(priority).expect("in range"),
            depth,
            action: Id::new(action).expect("valid id"),
        }
    }

    #[test]
    fn cheaper_expands_first() {
        assert!(entry(1, 5, "A002") < entry(2, 1, "A001"));
    }

    #[test]
    fn equal_cost_prefers_shallower() {
        assert!(entry(1, 1, "A002") < entry(1, 2, "A001"));
    }

    #[test]
    fn remaining_ties_break_on_action_identifier() {
        assert!(entry(1, 1, "A001") < entry(1, 1, "A002"));
    }

    #[test]
    fn ordering_is_independent_of_insertion_order() {
        let build = |entries: [FrontierEntry; 3]| {
            let mut sorted = entries.to_vec();
            sorted.sort();
            sorted
                .iter()
                .map(|e| e.action.to_string())
                .collect::<Vec<_>>()
        };

        let forward = build([
            entry(1, 1, "A001"),
            entry(1, 1, "A002"),
            entry(0, 9, "A003"),
        ]);
        let reverse = build([
            entry(0, 9, "A003"),
            entry(1, 1, "A002"),
            entry(1, 1, "A001"),
        ]);
        assert_eq!(forward, reverse);
        assert_eq!(forward, ["A003", "A001", "A002"]);
    }
}
