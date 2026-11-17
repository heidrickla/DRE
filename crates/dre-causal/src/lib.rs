//! Phase 7 — Causal reasoner.
//!
//! Represents actions as deterministic state transformations:
//!
//! ```text
//! S' = T_a(S)
//! ```
//!
//! Effects are total functions of the prior state. Probabilistic effects
//! are deferred (plan section 13): a stochastic transition would force the
//! planner to reason over distributions, and the MVP's whole question is
//! whether deterministic composition suffices.
//!
//! Applying an action to a state whose relevant properties are `UNKNOWN`
//! is a modelling question the engine should not paper over — an effect
//! of `temperature: add -200` has no defined result when the temperature
//! was never observed. The intended resolution is that such an action is
//! not applicable rather than silently producing an unknown result.
//!
//! Status: skeleton. Types are declared; application and applicability
//! checking are not implemented.

pub mod apply;

pub use apply::{applicable, apply, ApplyError, CausalSection};

use dre_core::collections::DetMap;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_state::Value;
use dre_symbolic::Condition;
use serde::{Deserialize, Serialize};

/// How an action changes one property.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Replace the value outright.
    Set(Value),
    /// Add a delta to a numeric value. Requires the prior value to be
    /// known; see the module note.
    Add(Fixed),
    /// Return the property to `UNKNOWN`, for actions that invalidate a
    /// prior observation rather than changing the world.
    Clear,
}

/// A deterministic state transformation.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// Unique identifier. Also the planner's final tie-break.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// Conditions that must hold for the action to be applicable.
    #[serde(default, rename = "preconditions")]
    pub preconditions: Vec<Condition>,
    /// Property changes, ordered by property.
    #[serde(default)]
    pub effects: DetMap<Id, Effect>,
    /// Cost used by the planner. Fixed-point, so path costs accumulate
    /// exactly and two paths that should tie actually do.
    #[serde(default = "unit_cost")]
    pub cost: Fixed,
    /// Whether an inverse action exists.
    #[serde(default)]
    pub reversible: bool,
}

fn unit_cost() -> Fixed {
    Fixed::ONE
}
