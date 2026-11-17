//! A\* must find the same plan as Dijkstra, only faster.
//!
//! This is the property that matters, and it is stronger than "both are
//! optimal". Several routes are equally optimal on this grid, so two
//! strategies could each return a shortest path and still disagree about
//! *which* one — and then the strategy would be part of the decision, not
//! just of how long the decision took.
//!
//! If this test ever fails, the honest fix is not to change the assertion.
//! It is to record that the strategy has become part of the *decision*.
//!
//! # Where the original note was half right
//!
//! This file used to end that sentence with "record that `Strategy`
//! belongs in the `EngineHash`", as though engine identity were a question
//! about the decision alone. `Strategy` is in the `EngineHash` now and the
//! assertion below still passes, because the two are separate questions.
//!
//! Five hashes are verified, not one. A\* and Dijkstra agree on the answer
//! — measured on the 40-by-40 pack, in `scale.rs` — but A\* reaches it
//! having expanded 383 states where Dijkstra expands 727, and the proof
//! records every candidate rejected along the way. So the `ProofHash`
//! differs while the `DecisionHash` does not, and a strategy outside the
//! engine identity would leave that difference with nothing to explain it:
//! four hashes equal, one not, which reads as a determinism failure.
//!
//! The membership rule is therefore about any verified hash, not only the
//! decision. See `dre_core::config::Strategy`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::context::ExecutionContext;
use dre_core::id::Id;
use dre_model::{load_dir, parse_section, SectionKind};
use dre_planner::{plan, ConstraintsSection, GoalsSection, Strategy};
use dre_proof::Trace;
use dre_state::{StateBuilder, Value};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn navigation_pack() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("model-packs/navigation")
}

fn solve(strategy: Strategy) -> (Vec<String>, u64) {
    let pack = load_dir(&navigation_pack()).expect("navigation pack loads");

    let actions = parse_section::<dre_causal::CausalSection>(&pack, SectionKind::Causal)
        .expect("causal parses")
        .expect("pack declares actions")
        .actions;
    let goals = parse_section::<GoalsSection>(&pack, SectionKind::Goals)
        .expect("goals parse")
        .expect("pack declares goals")
        .goals;
    let constraints = parse_section::<ConstraintsSection>(&pack, SectionKind::Constraints)
        .expect("constraints parse")
        .unwrap_or_default()
        .constraints;

    let mut builder = StateBuilder::with_default_limits();
    builder.entity(id("agent"), id("agent")).expect("entity");
    builder
        .observe(&id("agent"), id("x"), Value::Integer(0))
        .expect("x");
    builder
        .observe(&id("agent"), id("y"), Value::Integer(0))
        .expect("y");

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();

    let found = plan(
        strategy,
        &actions,
        &goals[0],
        &constraints,
        builder.build(),
        &mut context,
        &mut trace,
    )
    .expect("search succeeds")
    .expect("a plan exists");

    (
        found.actions.iter().map(ToString::to_string).collect(),
        context.counters().states_expanded,
    )
}

#[test]
fn astar_returns_the_same_plan_as_dijkstra() {
    let (dijkstra, _) = solve(Strategy::Dijkstra);
    let (astar, _) = solve(Strategy::AStar);
    assert_eq!(
        astar, dijkstra,
        "A* and Dijkstra disagreed on which optimal plan to return, \
         which would make Strategy part of the decision"
    );
}

#[test]
fn astar_never_expands_more_than_dijkstra() {
    let (_, dijkstra_states) = solve(Strategy::Dijkstra);
    let (_, astar_states) = solve(Strategy::AStar);

    // The guarantee that actually holds. On this pack the two are *equal*:
    // a 5-by-5 grid has about 22 reachable cells and both searches visit
    // nearly all of them, so there is no room for guidance to pay off.
    // That is a property of the pack, not a defect in the heuristic —
    // `astar_guides_when_the_space_is_large_enough` is where it earns its
    // keep. Asserting strict improvement here would be asserting something
    // about the grid size.
    assert!(
        astar_states <= dijkstra_states,
        "A* expanded {astar_states}, Dijkstra {dijkstra_states} — a heuristic must never make it worse"
    );
}

/// A clear grid of `size` by `size` with unit-cost moves, built in code.
///
/// The domain pack stays a description of its domain; this is a scratch
/// space for testing search behaviour that needs more room than the pack
/// has.
fn open_grid(bound: i64, goal_at: i64) -> (Vec<dre_causal::Action>, dre_planner::Goal) {
    use dre_causal::{Action, Effect};
    use dre_core::collections::DetMap;
    use dre_core::fixed::Fixed;
    use dre_symbolic::{Condition, Operator};

    let axis_action = |name: &str, axis: &str, delta: i64, guard: Operator, bound: i64| {
        let mut effects = DetMap::new();
        effects.insert(
            id(&format!("agent.{axis}")),
            Effect::Add(Fixed::from_int(delta).expect("in range")),
        );
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
    };

    let actions = vec![
        axis_action("MOVE_EAST", "x", 1, Operator::Lte, bound - 1),
        axis_action("MOVE_NORTH", "y", 1, Operator::Lte, bound - 1),
        axis_action("MOVE_SOUTH", "y", -1, Operator::Gte, 1),
        axis_action("MOVE_WEST", "x", -1, Operator::Gte, 1),
    ];

    let goal = dre_planner::Goal {
        id: id("CORNER"),
        description: String::new(),
        conditions: vec![
            Condition::Compare {
                subject: id("agent.x"),
                operator: Operator::Eq,
                operand: Value::Integer(goal_at),
            },
            Condition::Compare {
                subject: id("agent.y"),
                operator: Operator::Eq,
                operand: Value::Integer(goal_at),
            },
        ],
    };

    (actions, goal)
}

fn solve_open_grid(strategy: Strategy, bound: i64, goal_at: i64) -> (usize, u64) {
    let (actions, goal) = open_grid(bound, goal_at);

    let mut builder = StateBuilder::with_default_limits();
    builder.entity(id("agent"), id("agent")).expect("entity");
    builder
        .observe(&id("agent"), id("x"), Value::Integer(0))
        .expect("x");
    builder
        .observe(&id("agent"), id("y"), Value::Integer(0))
        .expect("y");

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();

    let found = plan(
        strategy,
        &actions,
        &goal,
        &[],
        builder.build(),
        &mut context,
        &mut trace,
    )
    .expect("search succeeds")
    .expect("a plan exists");

    (found.actions.len(), context.counters().states_expanded)
}

#[test]
fn astar_guides_when_the_space_is_large_enough() {
    // A clear 9-by-9 grid with the goal at (4,4) — deliberately *not* the
    // far corner.
    //
    // With the goal at the opposite corner every cell lies on some
    // shortest path, so g + h is the same constant everywhere and the
    // heuristic cannot discriminate at all: A* degenerates to Dijkstra and
    // both expand all 100 cells. That is a real property of Manhattan
    // distance on an open grid, not a defect, and it is worth knowing —
    // a heuristic only helps when there are states it can rank as worse.
    //
    // Putting the goal mid-grid gives the cells beyond it a higher f, so
    // there is finally something for A* to avoid.
    let (dijkstra_len, dijkstra_states) = solve_open_grid(Strategy::Dijkstra, 9, 4);
    let (astar_len, astar_states) = solve_open_grid(Strategy::AStar, 9, 4);

    assert_eq!(dijkstra_len, 8, "Manhattan distance from (0,0) to (4,4)");
    assert_eq!(astar_len, 8, "A* must still return an optimal plan");

    assert!(
        astar_states < dijkstra_states,
        "A* expanded {astar_states}, Dijkstra {dijkstra_states} — the heuristic is not guiding"
    );
}

#[test]
fn both_strategies_are_reproducible() {
    for strategy in [Strategy::Dijkstra, Strategy::AStar, Strategy::BreadthFirst] {
        let first = solve(strategy);
        let second = solve(strategy);
        assert_eq!(first, second, "{strategy:?} was not reproducible");
    }
}

#[test]
fn every_strategy_finds_an_optimal_length_route() {
    // Breadth-first optimises depth rather than cost, so it need not agree
    // on *which* route — but with unit costs it must still find one of
    // length 8.
    for strategy in [Strategy::Dijkstra, Strategy::AStar, Strategy::BreadthFirst] {
        let (route, _) = solve(strategy);
        assert_eq!(
            route.len(),
            8,
            "{strategy:?} returned {} steps",
            route.len()
        );
    }
}

// ---------------------------------------------------------------------
// Admissibility
//
// A* only returns optimal plans when the heuristic never overestimates.
// Nothing about a suboptimal plan looks wrong from the outside — it is a
// valid sequence that reaches the goal — so the only place the failure can
// be caught is before the search starts.
// ---------------------------------------------------------------------

/// One action, modelled to order, plus the two-dimension grid goal.
fn action_with(
    name: &str,
    effects: Vec<(&str, dre_causal::Effect)>,
    cost: i64,
) -> dre_causal::Action {
    use dre_core::collections::DetMap;
    use dre_core::fixed::Fixed;

    let mut map = DetMap::new();
    for (subject, effect) in effects {
        map.insert(id(subject), effect);
    }

    dre_causal::Action {
        id: id(name),
        description: String::new(),
        preconditions: Vec::new(),
        effects: map,
        cost: Fixed::from_int(cost).expect("in range"),
        reversible: false,
    }
}

fn grid_goal() -> dre_planner::Goal {
    let (_, goal) = open_grid(9, 4);
    goal
}

#[test]
fn the_navigation_pack_satisfies_the_heuristic_it_is_planned_with() {
    // The check earns nothing if the shipped pack does not pass it.
    let pack = load_dir(&navigation_pack()).expect("navigation pack loads");
    let actions = parse_section::<dre_causal::CausalSection>(&pack, SectionKind::Causal)
        .expect("causal parses")
        .expect("pack declares actions")
        .actions;
    let goals = parse_section::<GoalsSection>(&pack, SectionKind::Goals)
        .expect("goals parse")
        .expect("pack declares goals")
        .goals;

    assert!(dre_planner::check_admissible(&actions, &goals[0]).is_ok());
}

#[test]
fn an_action_that_outruns_its_cost_is_refused() {
    use dre_causal::Effect;
    use dre_core::fixed::Fixed;

    // Two units of progress for one unit of cost. Manhattan then reads a
    // distance of 8 where the true remaining cost is 4, and A* would
    // happily return a longer route believing it was cheaper.
    let leaping = action_with(
        "LEAP_EAST",
        vec![(
            "agent.x",
            Effect::Add(Fixed::from_int(2).expect("in range")),
        )],
        1,
    );

    let error = dre_planner::check_admissible(&[leaping], &grid_goal())
        .expect_err("a two-unit step at unit cost is not admissible");

    assert!(
        matches!(&error, dre_planner::SearchError::Inadmissible { action, .. }
            if action.as_str() == "LEAP_EAST"),
        "the error must name the offending action, got {error}"
    );
}

#[test]
fn the_same_action_is_fine_once_it_pays_for_itself() {
    use dre_causal::Effect;
    use dre_core::fixed::Fixed;

    // Not a rule against long steps — a rule about the ratio. Charge two
    // for two units of movement and the estimate is exact again.
    let paid = action_with(
        "LEAP_EAST",
        vec![(
            "agent.x",
            Effect::Add(Fixed::from_int(2).expect("in range")),
        )],
        2,
    );

    assert!(dre_planner::check_admissible(&[paid], &grid_goal()).is_ok());
}

#[test]
fn moving_two_dimensions_at_once_counts_as_two_units() {
    use dre_causal::Effect;
    use dre_core::fixed::Fixed;

    // The case a per-dimension check would wave through: each axis moves
    // only one unit, but Manhattan sums the axes, so a diagonal step at
    // unit cost still overestimates.
    let one = Fixed::from_int(1).expect("in range");
    let diagonal = action_with(
        "MOVE_NORTHEAST",
        vec![("agent.x", Effect::Add(one)), ("agent.y", Effect::Add(one))],
        1,
    );

    assert!(
        dre_planner::check_admissible(&[diagonal], &grid_goal()).is_err(),
        "a diagonal step covers two units of Manhattan distance for one unit of cost"
    );
}

#[test]
fn setting_a_goal_dimension_outright_is_never_admissible() {
    use dre_causal::Effect;

    // A `set` can close any distance at all in one step, so no finite cost
    // makes the estimate safe. There is no threshold to compare against.
    let teleport = action_with(
        "TELEPORT",
        vec![("agent.x", Effect::Set(Value::Integer(4)))],
        1_000,
    );

    assert!(dre_planner::check_admissible(&[teleport], &grid_goal()).is_err());
}

#[test]
fn clearing_a_goal_dimension_is_safe() {
    use dre_causal::Effect;

    // Clearing returns the property to UNKNOWN, which the heuristic skips.
    // The estimate can only fall, and an underestimate is admissible.
    let forget = action_with("FORGET_X", vec![("agent.x", Effect::Clear)], 1);

    assert!(dre_planner::check_admissible(&[forget], &grid_goal()).is_ok());
}

#[test]
fn an_action_that_misses_the_goal_dimensions_is_irrelevant() {
    use dre_causal::Effect;
    use dre_core::fixed::Fixed;

    let unrelated = action_with(
        "REFUEL",
        vec![(
            "agent.fuel",
            Effect::Add(Fixed::from_int(50).expect("in range")),
        )],
        1,
    );

    assert!(dre_planner::check_admissible(&[unrelated], &grid_goal()).is_ok());
}

#[test]
fn astar_refuses_an_inadmissible_model_but_dijkstra_still_plans() {
    use dre_causal::Effect;
    use dre_core::fixed::Fixed;
    use dre_proof::Trace;

    // The distinction that makes the check worth having: the model is not
    // broken, it just cannot be searched with this heuristic. Dijkstra has
    // no heuristic, so it is unaffected and still returns an optimal plan.
    let mut actions = open_grid(9, 4).0;
    actions.push(action_with(
        "LEAP_EAST",
        vec![(
            "agent.x",
            Effect::Add(Fixed::from_int(2).expect("in range")),
        )],
        1,
    ));
    let goal = grid_goal();

    let start = || {
        let mut builder = StateBuilder::with_default_limits();
        builder.entity(id("agent"), id("agent")).expect("entity");
        builder
            .observe(&id("agent"), id("x"), Value::Integer(0))
            .expect("x");
        builder
            .observe(&id("agent"), id("y"), Value::Integer(0))
            .expect("y");
        builder.build()
    };

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let refused = plan(
        Strategy::AStar,
        &actions,
        &goal,
        &[],
        start(),
        &mut context,
        &mut trace,
    );
    assert!(
        matches!(refused, Err(dre_planner::SearchError::Inadmissible { .. })),
        "A* must refuse rather than return a plan it cannot vouch for"
    );

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let found = plan(
        Strategy::Dijkstra,
        &actions,
        &goal,
        &[],
        start(),
        &mut context,
        &mut trace,
    )
    .expect("Dijkstra is unaffected")
    .expect("a plan exists");

    // Two leaps east and four steps north beats eight unit moves.
    assert_eq!(found.actions.len(), 6);
}

#[test]
fn a_goal_with_no_numeric_target_is_vacuously_admissible() {
    use dre_causal::Effect;
    use dre_symbolic::{Condition, Operator};

    // With nothing to measure, the heuristic is a constant zero, which
    // never overestimates anything.
    let goal = dre_planner::Goal {
        id: id("ARRIVED"),
        description: String::new(),
        conditions: vec![Condition::Status {
            subject: id("agent.docked"),
            operator: Operator::Known,
        }],
    };

    let teleport = action_with(
        "TELEPORT",
        vec![("agent.x", Effect::Set(Value::Integer(4)))],
        1,
    );

    assert!(dre_planner::check_admissible(&[teleport], &goal).is_ok());
}
