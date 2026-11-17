//! What A\* is worth at a scale nobody can check by hand.
//!
//! `astar_matches_dijkstra` establishes the two agree, on a 5-by-5 pack and
//! on a code-built 9-by-9 grid. Both are small enough to reason about
//! directly, which is their virtue and also their limit: the roadmap kept
//! "A\* is unmeasured at scale" open precisely because agreement on twenty
//! reachable cells says little about sixteen hundred.
//!
//! Two goals are measured here, and the pair is the point.
//!
//! At the far corner the heuristic is not weak, it is *inert*: every cell
//! lies on some shortest path from the origin, so `g + h` is constant and
//! there is nothing to rank as worse. A\* and Dijkstra expand the identical
//! state count. Measuring A\* there and reporting "no improvement" would be
//! measuring the goal placement, not the heuristic.
//!
//! Away from the corner the cells beyond the goal lie on no shortest path,
//! `g + h` ranks them worse, and A\* declines to expand them. That is where
//! the guidance shows.
//!
//! These are exact equalities rather than bounds. The counts are a
//! deterministic function of the pack, the frontier order and the
//! heuristic, so a range would only hide the thing worth noticing — that
//! one of the three moved.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::config::{Limits, RuntimeConfig};
use dre_core::context::ExecutionContext;
use dre_core::id::Id;
use dre_model::{load_dir, parse_section, SectionKind};
use dre_planner::{plan, ConstraintsSection, GoalsSection, Strategy};
use dre_proof::Trace;
use dre_state::{StateBuilder, Value};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn pack_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("model-packs")
        .join(name)
}

/// Plan across one of the generated grid packs.
///
/// Returns the route and the states expanded reaching it.
fn solve(pack_name: &str, strategy: Strategy, max_depth: u64) -> (Vec<String>, u64) {
    let pack = load_dir(&pack_dir(pack_name)).expect("grid pack loads");

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

    // The same ceiling the matching experiment declares. A 40-wide grid
    // does not fit inside the default depth of 20, and raising it here
    // rather than defaulting keeps this test asking the question the
    // experiment asks.
    let config = RuntimeConfig {
        limits: Limits {
            max_depth,
            ..Limits::MVP
        },
        ..RuntimeConfig::default()
    };
    let mut context = ExecutionContext::new(config).expect("context");
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
fn at_the_far_corner_the_heuristic_is_inert() {
    let (dijkstra, dijkstra_states) = solve("grid-40", Strategy::Dijkstra, 86);
    let (astar, astar_states) = solve("grid-40", Strategy::AStar, 86);

    assert_eq!(
        dijkstra.len(),
        78,
        "Manhattan distance from (0,0) to (39,39)"
    );
    assert_eq!(astar, dijkstra, "A* must return the same optimal plan");

    // Not "no worse" — identical. Every cell lies on a shortest path, so
    // the heuristic ranks nothing below anything else and the two searches
    // do exactly the same work. If this ever becomes an inequality the
    // heuristic has started discriminating on a goal where it provably
    // cannot, and the heuristic is what changed.
    assert_eq!(
        astar_states, dijkstra_states,
        "A* expanded {astar_states}, Dijkstra {dijkstra_states} — at the far \
         corner g + h is constant and they must do identical work"
    );
    assert_eq!(dijkstra_states, 1518);
}

#[test]
fn away_from_the_corner_the_heuristic_halves_the_search() {
    let (dijkstra, dijkstra_states) = solve("grid-40-mid", Strategy::Dijkstra, 46);
    let (astar, astar_states) = solve("grid-40-mid", Strategy::AStar, 46);

    assert_eq!(
        dijkstra.len(),
        38,
        "Manhattan distance from (0,0) to (19,19)"
    );
    assert_eq!(astar, dijkstra, "A* must return the same optimal plan");

    assert_eq!(dijkstra_states, 727);
    assert_eq!(astar_states, 383);
}

#[test]
fn the_two_strategies_agree_on_the_answer_and_differ_on_the_work() {
    // The finding that put `Strategy` in the `EngineHash`, stated as one
    // assertion. Same plan, different search — so a run's `DecisionHash`
    // is independent of the strategy while its `ProofHash` is not, and an
    // engine identity that ignored the strategy would leave the proof
    // difference looking like non-determinism.
    let (dijkstra, dijkstra_states) = solve("grid-40-mid", Strategy::Dijkstra, 46);
    let (astar, astar_states) = solve("grid-40-mid", Strategy::AStar, 46);

    assert_eq!(astar, dijkstra);
    assert_ne!(astar_states, dijkstra_states);
}
