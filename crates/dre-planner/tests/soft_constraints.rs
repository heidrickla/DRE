//! A soft constraint is a price, not a prohibition.
//!
//! The navigation pack declares a hazard at (3,3) worth 5, and the planner
//! routes around it — at no extra cost, because an equal-length detour
//! exists. So the pack demonstrates the "avoid" half and never once pays
//! the penalty. Everything interesting about a soft constraint is in the
//! other half: what happens when avoiding it is *more* expensive than
//! crossing it.
//!
//! These grids are built in code rather than added to the pack. The pack
//! is a description of its domain and should not grow cells that exist
//! only to make a test tip one way; the tipping point itself is engine
//! behaviour and belongs here.
//!
//! # The arithmetic being tested
//!
//! Start (0,0), goal (4,0), hazard at (2,0):
//!
//! ```text
//!   y=1   . → → → .
//!         ↑       ↓
//!   y=0   S   X   . → G
//!         0 1 2 3 4
//! ```
//!
//! - Straight through: 4 moves, one hazard crossing → `4 + penalty`
//! - Around it: 6 moves, no crossing → `6`
//!
//! Any deviation from a Manhattan-shortest path on an open grid costs two,
//! so 6 is the cheapest bypass there is. The planner should therefore cross
//! below a penalty of 2 and detour above it — and the boundary case is the
//! one worth pinning down, because at exactly 2 the two routes tie and
//! something has to break it the same way every time.

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

fn fixed(value: i64) -> Fixed {
    Fixed::from_int(value).expect("in range")
}

/// One axis-aligned move, guarded so the agent stays on the grid.
fn move_action(name: &str, axis: &str, delta: i64, guard: Operator, bound: i64) -> Action {
    let mut effects = DetMap::new();
    effects.insert(id(&format!("agent.{axis}")), Effect::Add(fixed(delta)));

    Action {
        id: id(name),
        description: String::new(),
        preconditions: vec![Condition::Compare {
            subject: id(&format!("agent.{axis}")),
            operator: guard,
            operand: Value::Integer(bound),
        }],
        effects,
        cost: Fixed::ONE,
        reversible: true,
    }
}

/// A clear grid with unit-cost moves in all four directions.
fn grid(bound: i64) -> Vec<Action> {
    vec![
        move_action("MOVE_EAST", "x", 1, Operator::Lte, bound - 1),
        move_action("MOVE_NORTH", "y", 1, Operator::Lte, bound - 1),
        move_action("MOVE_SOUTH", "y", -1, Operator::Gte, 1),
        move_action("MOVE_WEST", "x", -1, Operator::Gte, 1),
    ]
}

/// A corridor: east and west only, so nothing can be routed around.
fn corridor(bound: i64) -> Vec<Action> {
    vec![
        move_action("MOVE_EAST", "x", 1, Operator::Lte, bound - 1),
        move_action("MOVE_WEST", "x", -1, Operator::Gte, 1),
    ]
}

fn at(x: i64, y: i64) -> CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    builder.entity(id("agent"), id("agent")).expect("entity");
    builder
        .observe(&id("agent"), id("x"), Value::Integer(x))
        .expect("x");
    builder
        .observe(&id("agent"), id("y"), Value::Integer(y))
        .expect("y");
    builder.build()
}

fn goal_at(x: i64, y: i64) -> Goal {
    Goal {
        id: id("ARRIVE"),
        description: String::new(),
        conditions: vec![
            Condition::Compare {
                subject: id("agent.x"),
                operator: Operator::Eq,
                operand: Value::Integer(x),
            },
            Condition::Compare {
                subject: id("agent.y"),
                operator: Operator::Eq,
                operand: Value::Integer(y),
            },
        ],
    }
}

fn goal_on_axis(x: i64) -> Goal {
    Goal {
        id: id("ARRIVE"),
        description: String::new(),
        conditions: vec![Condition::Compare {
            subject: id("agent.x"),
            operator: Operator::Eq,
            operand: Value::Integer(x),
        }],
    }
}

/// A hazard on one cell, worth `penalty` to occupy.
///
/// Phrased as `NOT (x EQ .. AND y EQ ..)` so the constraint reads false —
/// that is, violated — exactly when the agent is standing on it.
fn hazard_at(x: i64, y: i64, penalty: i64) -> Constraint {
    Constraint {
        id: id("HAZARD"),
        description: String::new(),
        hard: false,
        // Soft, so this is ignored; stated rather than defaulted so the
        // file does not imply a soft constraint has an undecidable policy.
        on_undecidable: Undecidable::Permit,
        penalty: fixed(penalty),
        conditions: vec![Condition::Not {
            not: Box::new(Condition::All {
                all: vec![
                    Condition::Compare {
                        subject: id("agent.x"),
                        operator: Operator::Eq,
                        operand: Value::Integer(x),
                    },
                    Condition::Compare {
                        subject: id("agent.y"),
                        operator: Operator::Eq,
                        operand: Value::Integer(y),
                    },
                ],
            }),
        }],
    }
}

/// A hazard covering a whole column, unavoidable in a corridor.
fn hazard_column(x: i64, penalty: i64) -> Constraint {
    Constraint {
        id: id("HAZARD"),
        description: String::new(),
        hard: false,
        // Soft, so this is ignored; stated rather than defaulted so the
        // file does not imply a soft constraint has an undecidable policy.
        on_undecidable: Undecidable::Permit,
        penalty: fixed(penalty),
        conditions: vec![Condition::Not {
            not: Box::new(Condition::Compare {
                subject: id("agent.x"),
                operator: Operator::Eq,
                operand: Value::Integer(x),
            }),
        }],
    }
}

/// Plan, returning the route and what it cost.
fn solve(
    strategy: Strategy,
    actions: &[Action],
    goal: &Goal,
    constraints: &[Constraint],
    start: CanonicalState,
) -> (Vec<String>, Fixed) {
    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();

    let found = plan(
        strategy,
        actions,
        goal,
        constraints,
        start,
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

// ---------------------------------------------------------------------
// The half the navigation pack does exercise.
// ---------------------------------------------------------------------

#[test]
fn a_cheap_detour_is_taken() {
    // Penalty 5 against a detour worth 2. Avoiding is obviously correct
    // and this is the case the shipped pack already covers; it is here as
    // the control for the tests below.
    let (route, cost) = solve(
        Strategy::Dijkstra,
        &grid(5),
        &goal_at(4, 0),
        &[hazard_at(2, 0, 5)],
        at(0, 0),
    );

    assert_eq!(cost, fixed(6), "the six-step bypass, penalty unpaid");
    assert_eq!(route.len(), 6);
    assert!(
        !route.is_empty() && route.iter().filter(|step| *step == "MOVE_EAST").count() == 4,
        "still four net eastward moves: {route:?}"
    );
}

// ---------------------------------------------------------------------
// The half it does not.
// ---------------------------------------------------------------------

#[test]
fn an_expensive_detour_is_refused_and_the_penalty_is_paid() {
    // Penalty 1 against a detour worth 2. Crossing is now the cheaper
    // route, and a planner that treated the constraint as a prohibition
    // would return the six-step bypass instead — valid, obedient, wrong.
    let (route, cost) = solve(
        Strategy::Dijkstra,
        &grid(5),
        &goal_at(4, 0),
        &[hazard_at(2, 0, 1)],
        at(0, 0),
    );

    assert_eq!(route, ["MOVE_EAST"; 4], "straight through the hazard");
    assert_eq!(
        cost,
        fixed(5),
        "four moves plus the penalty it chose to pay"
    );
}

#[test]
fn the_tipping_point_is_where_the_arithmetic_says() {
    // The claim under test is not "sometimes it detours" but that the
    // choice is decided by cost, at the exact value where the two routes
    // cost the same. Below 2 the four-step crossing wins; above it, the
    // six-step bypass.
    for penalty in [0, 1] {
        let (route, cost) = solve(
            Strategy::Dijkstra,
            &grid(5),
            &goal_at(4, 0),
            &[hazard_at(2, 0, penalty)],
            at(0, 0),
        );
        assert_eq!(route.len(), 4, "penalty {penalty} should not buy a detour");
        assert_eq!(cost, fixed(4 + penalty));
    }

    for penalty in [3, 4, 50] {
        let (route, cost) = solve(
            Strategy::Dijkstra,
            &grid(5),
            &goal_at(4, 0),
            &[hazard_at(2, 0, penalty)],
            at(0, 0),
        );
        assert_eq!(route.len(), 6, "penalty {penalty} should buy the detour");
        assert_eq!(
            cost,
            fixed(6),
            "the bypass costs six however large the penalty"
        );
    }
}

#[test]
fn a_tie_between_crossing_and_detouring_resolves_the_same_way_every_time() {
    // Penalty exactly 2: crossing costs 4 + 2, the bypass costs 6. This is
    // the case that would expose a frontier order resolved by heap layout
    // rather than by the state hash, and it is not a corner case -- equal
    // costs are the normal situation on a grid.
    let tied = || {
        solve(
            Strategy::Dijkstra,
            &grid(5),
            &goal_at(4, 0),
            &[hazard_at(2, 0, 2)],
            at(0, 0),
        )
    };

    let (reference, cost) = tied();
    assert_eq!(cost, fixed(6), "both routes cost six, which is the point");

    for attempt in 0..32 {
        let (route, repeated) = tied();
        assert_eq!(
            route, reference,
            "attempt {attempt} chose a different route"
        );
        assert_eq!(repeated, cost);
    }
}

#[test]
fn an_unavoidable_hazard_is_crossed_rather_than_failing() {
    // A corridor has no bypass, so the penalty is not a choice. The
    // planner must still return the route: a soft constraint that made an
    // unavoidable cell unreachable would be a hard constraint wearing the
    // wrong label, and the search would report no path where one exists.
    let (route, cost) = solve(
        Strategy::Dijkstra,
        &corridor(5),
        &goal_on_axis(4),
        &[hazard_column(2, 100)],
        at(0, 0),
    );

    assert_eq!(route, ["MOVE_EAST"; 4]);
    assert_eq!(
        cost,
        fixed(104),
        "four moves and a penalty it could not avoid"
    );
}

#[test]
fn the_penalty_is_charged_once_per_crossing_not_once_per_plan() {
    // Two hazard cells on the same straight route. If the charge were per
    // plan, this would cost the same as crossing one -- and a route that
    // passed through five hazards would look as good as one that passed
    // through a single hazard.
    let (route, cost) = solve(
        Strategy::Dijkstra,
        &corridor(5),
        &goal_on_axis(4),
        &[hazard_column(2, 10), {
            let mut second = hazard_column(3, 10);
            second.id = id("HAZARD-2");
            second
        }],
        at(0, 0),
    );

    assert_eq!(route, ["MOVE_EAST"; 4]);
    assert_eq!(cost, fixed(24), "four moves and two separate penalties");
}

#[test]
fn a_hazard_off_the_route_costs_nothing() {
    // The complement: a penalty that is never entered must not show up in
    // the cost at all. Without this, a plan's cost could carry a charge
    // for a cell it never visited.
    let (route, cost) = solve(
        Strategy::Dijkstra,
        &grid(5),
        &goal_at(4, 0),
        &[hazard_at(0, 4, 100)],
        at(0, 0),
    );

    assert_eq!(route, ["MOVE_EAST"; 4]);
    assert_eq!(
        cost,
        fixed(4),
        "no penalty for a cell the route never entered"
    );
}

#[test]
fn an_undecidable_hazard_is_not_charged() {
    // A constraint the state cannot decide must not be treated as
    // violated. Charging on undecidable would bias the search on missing
    // information -- the planner would steer away from cells it simply
    // knows nothing about, which is the geometric version of concluding
    // something from ignorance.
    let unknowable = Constraint {
        id: id("HAZARD"),
        description: String::new(),
        hard: false,
        // Soft, so this is ignored; stated rather than defaulted so the
        // file does not imply a soft constraint has an undecidable policy.
        on_undecidable: Undecidable::Permit,
        penalty: fixed(100),
        conditions: vec![Condition::Compare {
            subject: id("agent.radiation"),
            operator: Operator::Lte,
            operand: Value::Integer(10),
        }],
    };

    let (route, cost) = solve(
        Strategy::Dijkstra,
        &corridor(5),
        &goal_on_axis(4),
        &[unknowable],
        at(0, 0),
    );

    assert_eq!(route, ["MOVE_EAST"; 4]);
    assert_eq!(
        cost,
        fixed(4),
        "an undecidable constraint is not a violated one"
    );
}

#[test]
fn astar_and_dijkstra_price_the_hazard_identically() {
    // Penalties raise the true cost of a route, so the Manhattan estimate
    // can only underestimate further -- it stays admissible and A* must
    // reach the same answer. If this ever fails, the heuristic is reading
    // something the cost function is not.
    // (penalty, total cost): crossing costs 4 + penalty, the bypass 6.
    let cases = [(1, 5), (3, 6), (2, 6)];

    for (penalty, expected_cost) in cases {
        let dijkstra = solve(
            Strategy::Dijkstra,
            &grid(5),
            &goal_at(4, 0),
            &[hazard_at(2, 0, penalty)],
            at(0, 0),
        );
        let astar = solve(
            Strategy::AStar,
            &grid(5),
            &goal_at(4, 0),
            &[hazard_at(2, 0, penalty)],
            at(0, 0),
        );

        assert_eq!(
            dijkstra.1,
            fixed(expected_cost),
            "penalty {penalty}: unexpected Dijkstra cost"
        );
        assert_eq!(
            astar, dijkstra,
            "penalty {penalty}: A* and Dijkstra disagreed, which would make Strategy part of the decision"
        );
    }
}

#[test]
fn a_hard_constraint_still_forbids_what_a_soft_one_only_prices() {
    // The distinction the two kinds exist to draw. Same cell, same grid:
    // as a price the planner pays it when paying is cheaper, and as a
    // prohibition no amount of saved distance buys passage.
    let mut wall = hazard_at(2, 0, 1);
    wall.hard = true;

    let (route, cost) = solve(
        Strategy::Dijkstra,
        &grid(5),
        &goal_at(4, 0),
        &[wall],
        at(0, 0),
    );

    assert_eq!(route.len(), 6, "a wall is routed around at any price");
    assert_eq!(
        cost,
        fixed(6),
        "and no penalty is charged, because none was paid"
    );
}
