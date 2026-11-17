//! Dijkstra must return the cheapest plan, not merely a valid one.
//!
//! Found by an independently authored pack. Its author predicted a
//! five-step remediation plan costing 10 and the engine returned a
//! four-step one costing 11 — a plan that is valid, reaches the goal, and
//! is not the cheapest. Nothing downstream can tell the difference, which
//! is the same invisible failure mode `check_admissible` exists to prevent
//! for A\*.
//!
//! The cause is where the frontier marks a state as visited. Marking on
//! *generation* is only correct if the first route to reach a state is the
//! cheapest one, and in a uniform-cost search it is not: a state can be
//! generated early by an expensive single action and later reached by a
//! cheaper sequence, whose successor is then discarded as already seen.
//!
//! This file is the minimal shape of that, independent of any pack.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_causal::{Action, Effect};
use dre_core::collections::DetMap;
use dre_core::context::ExecutionContext;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_planner::{plan, Goal, Strategy};
use dre_proof::Trace;
use dre_state::{CanonicalState, StateBuilder, Value};
use dre_symbolic::{Condition, Operator};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn fixed(value: i64) -> Fixed {
    Fixed::from_int(value).expect("in range")
}

/// An action setting `widget.step` to `to`, optionally requiring `from`.
fn step(name: &str, from: Option<i64>, to: i64, cost: i64) -> Action {
    let mut effects = DetMap::new();
    effects.insert(id("widget.step"), Effect::Set(Value::Integer(to)));

    Action {
        id: id(name),
        description: String::new(),
        preconditions: from
            .map(|value| {
                vec![Condition::Compare {
                    subject: id("widget.step"),
                    operator: Operator::Eq,
                    operand: Value::Integer(value),
                }]
            })
            .unwrap_or_default(),
        effects,
        cost: fixed(cost),
        reversible: false,
    }
}

fn start() -> CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    builder.entity(id("widget"), id("widget")).expect("entity");
    builder
        .observe(&id("widget"), id("step"), Value::Integer(0))
        .expect("step");
    builder.build()
}

fn goal_at(value: i64) -> Goal {
    Goal {
        id: id("ARRIVED"),
        description: String::new(),
        conditions: vec![Condition::Compare {
            subject: id("widget.step"),
            operator: Operator::Eq,
            operand: Value::Integer(value),
        }],
    }
}

fn solve(strategy: Strategy, actions: &[Action]) -> (Vec<String>, Fixed) {
    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();

    let found = plan(
        strategy,
        actions,
        &goal_at(2),
        &[],
        start(),
        &mut context,
        &mut trace,
    )
    .expect("search succeeds")
    .expect("a plan exists");

    (
        found.actions.iter().map(ToString::to_string).collect(),
        found.cost,
    )
}

/// One expensive action reaches the goal directly; two cheap ones reach it
/// for less.
///
/// Action identifiers are chosen so the expensive route is *generated
/// first*: expansion considers actions in identifier order, so `A_DIRECT`
/// is offered before `B_HALFWAY`. That is the ordering under which
/// marking visited at generation time discards the cheaper route.
fn detour_is_cheaper() -> Vec<Action> {
    vec![
        step("A_DIRECT", None, 2, 5),
        step("B_HALFWAY", None, 1, 1),
        step("C_FINISH", Some(1), 2, 1),
    ]
}

#[test]
fn dijkstra_returns_the_cheapest_plan_not_the_first_one_found() {
    let (route, cost) = solve(Strategy::Dijkstra, &detour_is_cheaper());

    assert_eq!(
        cost,
        fixed(2),
        "the two-step route costs 2 and the direct one costs 5; got {route:?}"
    );
    assert_eq!(route, ["B_HALFWAY", "C_FINISH"]);
}

/// The same shape, expressed additively.
///
/// A\* cannot search the `set`-based version at all: `widget.step` is a
/// goal dimension, a `set` can close any distance at once, and
/// `check_admissible` refuses the model rather than returning a plan it
/// cannot vouch for. That refusal is correct, so the A\* case needs a
/// model whose heuristic is bounded — one unit of movement per unit of
/// cost, or better.
fn detour_is_cheaper_additively() -> Vec<Action> {
    fn shift(name: &str, from: Option<i64>, delta: i64, cost: i64) -> Action {
        let mut effects = DetMap::new();
        effects.insert(id("widget.step"), Effect::Add(fixed(delta)));
        Action {
            id: id(name),
            description: String::new(),
            preconditions: from
                .map(|value| {
                    vec![Condition::Compare {
                        subject: id("widget.step"),
                        operator: Operator::Eq,
                        operand: Value::Integer(value),
                    }]
                })
                .unwrap_or_default(),
            effects,
            cost: fixed(cost),
            reversible: false,
        }
    }

    vec![
        // Two units of progress for five units of cost: admissible,
        // because it charges more than it moves.
        shift("A_DIRECT", Some(0), 2, 5),
        shift("B_HALFWAY", Some(0), 1, 1),
        shift("C_FINISH", Some(1), 1, 1),
    ]
}

#[test]
fn astar_returns_the_cheapest_plan_too() {
    // The bug would reappear the moment a pack used A*, so it is worth
    // pinning separately rather than assuming the shared search covers it.
    let (route, cost) = solve(Strategy::AStar, &detour_is_cheaper_additively());
    assert_eq!(cost, fixed(2), "got {route:?}");
    assert_eq!(route, ["B_HALFWAY", "C_FINISH"]);
}

#[test]
fn astar_and_dijkstra_agree_on_the_cheapest_route() {
    // If these ever diverge, the honest fix is to record that `Strategy`
    // belongs in the `EngineHash`.
    assert_eq!(
        solve(Strategy::AStar, &detour_is_cheaper_additively()),
        solve(Strategy::Dijkstra, &detour_is_cheaper_additively())
    );
}

#[test]
fn breadth_first_still_counts_steps_rather_than_cost() {
    // The contrast that keeps the fix honest. BFS is *supposed* to return
    // the shallowest plan regardless of declared cost, so here it takes
    // the single expensive action -- and that is correct behaviour, not
    // the bug above.
    let (route, cost) = solve(Strategy::BreadthFirst, &detour_is_cheaper());
    assert_eq!(route, ["A_DIRECT"], "BFS wants the fewest steps");
    assert_eq!(cost, fixed(1), "BFS counts depth, not declared cost");
}

#[test]
fn a_long_cheap_chain_beats_a_short_expensive_one() {
    // Wider version: four one-cost steps against one action worth ten.
    // A search that stopped at the first route to touch the goal, or that
    // locked in the first way it reached each state, returns the ten.
    let mut actions = vec![step("A_LEAP", None, 4, 10)];
    for n in 0..4 {
        actions.push(step(
            &format!("B_STEP_{n}"),
            if n == 0 { None } else { Some(n) },
            n + 1,
            1,
        ));
    }

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let found = plan(
        Strategy::Dijkstra,
        &actions,
        &goal_at(4),
        &[],
        start(),
        &mut context,
        &mut trace,
    )
    .expect("search succeeds")
    .expect("a plan exists");

    assert_eq!(
        found.cost,
        fixed(4),
        "four cheap steps beat one expensive one"
    );
    assert_eq!(found.actions.len(), 4);
}

#[test]
fn the_cheaper_route_is_still_found_when_generated_last() {
    // The mirror image, so the fix is not simply an ordering accident.
    // Here the cheap route sorts *after* the expensive one is exhausted.
    let actions = vec![
        step("A_HALFWAY", None, 1, 1),
        step("B_FINISH", Some(1), 2, 1),
        step("C_DIRECT", None, 2, 5),
    ];

    let (route, cost) = solve(Strategy::Dijkstra, &actions);
    assert_eq!(cost, fixed(2));
    assert_eq!(route, ["A_HALFWAY", "B_FINISH"]);
}
