//! A hard constraint that cannot tell whether it is satisfied.
//!
//! Every other constraint test asks what happens when a constraint is
//! decidably true or decidably false. This one asks the third question,
//! which three-valued logic makes unavoidable and which the planner
//! previously answered the same way every time: **permit**.
//!
//! That single answer is wrong for half of all constraints. A safety
//! invariant exists precisely for the case where information is missing,
//! and permitting on undecidable means the one situation it was written
//! for is the one situation it does not cover. Equally, a procedural
//! invariant that blocks on undecidable prunes every candidate before the
//! facts that would satisfy it have been established, and the planner
//! reports no path where one plainly exists.
//!
//! So the reading is the constraint's to declare. These tests pin both
//! halves, and the shape of each is the same: an identical problem, an
//! identical constraint, differing only in `on_undecidable`, reaching two
//! different answers — and each answer being the right one for its case.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_causal::{Action, Effect};
use dre_core::collections::DetMap;
use dre_core::context::ExecutionContext;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_planner::{plan, Constraint, Goal, Strategy, Undecidable};
use dre_proof::Trace;
use dre_state::{CanonicalState, StateBuilder, Value};
use dre_symbolic::{Condition, Operator};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

/// A single action that reaches the goal in one step.
fn step() -> Vec<Action> {
    let mut effects = DetMap::new();
    effects.insert(id("agent.arrived"), Effect::Set(Value::Boolean(true)));

    vec![Action {
        id: id("ADVANCE"),
        description: String::new(),
        preconditions: Vec::new(),
        effects,
        cost: Fixed::ONE,
        reversible: false,
    }]
}

fn goal() -> Goal {
    Goal {
        id: id("ARRIVE"),
        description: String::new(),
        conditions: vec![Condition::Compare {
            subject: id("agent.arrived"),
            operator: Operator::Eq,
            operand: Value::Boolean(true),
        }],
    }
}

/// State in which `agent.arrived` is the only property that exists.
///
/// `agent.interlock_engaged` is deliberately absent, so any constraint
/// reading it evaluates to `Unknown` — not false, and not true.
fn start() -> CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    builder.entity(id("agent"), id("agent")).expect("entity");
    builder
        .observe(&id("agent"), id("arrived"), Value::Boolean(false))
        .expect("arrived");
    builder.build()
}

/// A hard constraint on a property that is not in the state.
fn interlock(on_undecidable: Undecidable) -> Constraint {
    Constraint {
        id: id("INTERLOCK"),
        description: String::new(),
        conditions: vec![Condition::Compare {
            subject: id("agent.interlock_engaged"),
            operator: Operator::Eq,
            operand: Value::Boolean(true),
        }],
        hard: true,
        penalty: Fixed::ONE,
        on_undecidable,
    }
}

fn attempt(constraints: &[Constraint]) -> Option<dre_planner::Plan> {
    let mut context = ExecutionContext::with_defaults().expect("valid config");
    let mut trace = Trace::new();

    plan(
        Strategy::Dijkstra,
        &step(),
        &goal(),
        constraints,
        start(),
        &mut context,
        &mut trace,
    )
    .expect("search completes")
}

// ---------------------------------------------------------------------
// The two readings.
// ---------------------------------------------------------------------

#[test]
fn the_control_case_reaches_the_goal_with_no_constraints() {
    // Without this, a test below could pass because the problem is
    // unsolvable rather than because a constraint blocked it.
    let found = attempt(&[]).expect("the goal is one step away");
    assert_eq!(found.actions.len(), 1);
}

#[test]
fn blocking_is_the_default() {
    // Not merely "blocking works" — that the *absence* of a declaration
    // gives the safe reading. A pack author who has not thought about
    // undecidable constraints should get the answer that does not quietly
    // grant permission.
    let default_constraint = interlock(Undecidable::default());
    assert!(attempt(&[default_constraint]).is_none());
}

#[test]
fn a_blocking_constraint_stops_a_plan_it_cannot_clear() {
    assert!(
        attempt(&[interlock(Undecidable::Block)]).is_none(),
        "an interlock that cannot be confirmed must not be treated as confirmed"
    );
}

#[test]
fn a_permitting_constraint_lets_the_same_plan_through() {
    let found = attempt(&[interlock(Undecidable::Permit)])
        .expect("permitting on undecidable must leave the candidate alone");
    assert_eq!(found.actions.len(), 1);
}

#[test]
fn the_two_readings_disagree_on_the_same_problem() {
    // The whole point, stated as one assertion: same actions, same goal,
    // same state, same constraint — and a different decision. If these
    // ever agree, the declaration has stopped meaning anything.
    assert_ne!(
        attempt(&[interlock(Undecidable::Block)]).is_some(),
        attempt(&[interlock(Undecidable::Permit)]).is_some()
    );
}

// ---------------------------------------------------------------------
// What the setting must not change.
// ---------------------------------------------------------------------

#[test]
fn a_decidably_satisfied_constraint_permits_under_either_reading() {
    // `agent.arrived` is in the state, so `EXISTS` has a definite answer.
    // Neither reading may touch a constraint that has one.
    let decidable = |on_undecidable| Constraint {
        id: id("STAYS_TRUE"),
        description: String::new(),
        conditions: vec![Condition::Status {
            subject: id("agent.arrived"),
            operator: Operator::Exists,
        }],
        hard: true,
        penalty: Fixed::ONE,
        on_undecidable,
    };

    assert!(attempt(&[decidable(Undecidable::Block)]).is_some());
    assert!(attempt(&[decidable(Undecidable::Permit)]).is_some());
}

#[test]
fn a_decidably_violated_constraint_blocks_under_either_reading() {
    // Blocking on false is not the setting's business, and a `permit`
    // declaration must not be readable as "this constraint is advisory".
    let forbidden = |on_undecidable| Constraint {
        id: id("NEVER"),
        description: String::new(),
        conditions: vec![Condition::Status {
            subject: id("agent.nonexistent"),
            operator: Operator::Exists,
        }],
        hard: true,
        penalty: Fixed::ONE,
        on_undecidable,
    };

    assert!(attempt(&[forbidden(Undecidable::Block)]).is_none());
    assert!(
        attempt(&[forbidden(Undecidable::Permit)]).is_none(),
        "`permit` governs undecidable, not false"
    );
}

#[test]
fn a_soft_constraint_is_unaffected() {
    // Soft constraints are priced, not enforced. An undecidable one
    // charges nothing under either declaration, because charging for a
    // violation the model never established would bias the search on
    // missing information rather than on the model.
    let soft = |on_undecidable| Constraint {
        id: id("PREFER"),
        description: String::new(),
        conditions: vec![Condition::Compare {
            subject: id("agent.interlock_engaged"),
            operator: Operator::Eq,
            operand: Value::Boolean(true),
        }],
        hard: false,
        penalty: Fixed::from_int(5).expect("in range"),
        on_undecidable,
    };

    let blocking = attempt(&[soft(Undecidable::Block)]).expect("a soft constraint never blocks");
    let permitting = attempt(&[soft(Undecidable::Permit)]).expect("nor under the other reading");

    assert_eq!(blocking.cost, permitting.cost);
    assert_eq!(
        blocking.cost,
        Fixed::ONE,
        "an undecidable soft constraint must not be charged for"
    );
}
