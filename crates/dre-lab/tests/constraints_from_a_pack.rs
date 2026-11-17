//! The constraint layer, exercised by a model rather than by a fixture.
//!
//! `soft_constraints.rs` and `undecidable_constraints.rs` already prove the
//! arithmetic and the three-valued logic, on grids built in code. They are
//! the right place for a tipping point, and their own header explains why:
//! a pack is a description of its domain and should not grow cells that
//! exist only to make a test tip one way.
//!
//! What they cannot prove is that a pack author can reach any of it. That
//! distinction has produced most of the defects in this repository — four
//! of eight resource ceilings enforced nothing, three of five geometry
//! operators had no caller, `features.yaml` was declared and unread, a
//! geometric space could only ever classify one entity. Each was implemented,
//! tested, and unreachable.
//!
//! Two constraint behaviours were in that state until `haulage` existed:
//!
//! - **A soft penalty had never been paid.** The navigation pack's hazard is
//!   routed around at no extra cost, because an equal-length detour exists,
//!   so every shipped plan had avoided a hazard for free.
//! - **No shipped pack had a hard constraint that could go undecidable**, so
//!   `on_undecidable` — whose default was deliberately changed to `block` —
//!   rested entirely on its own unit test.
//!
//! `haulage` is a domain where both are intrinsic rather than contrived: two
//! water crossings nobody has rated, and a ford that is worth using but not
//! worth using freely.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::fixed::Fixed;
use dre_lab::runner;

fn run(name: &str) -> runner::RunResult {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("experiments")
        .join(name);
    runner::run(&path).unwrap_or_else(|error| panic!("{name} must run: {error}"))
}

fn route(result: &runner::RunResult) -> Vec<String> {
    result
        .plan
        .as_ref()
        .expect("a plan must be produced")
        .actions
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn cost(result: &runner::RunResult) -> Fixed {
    result.plan.as_ref().expect("a plan").cost
}

#[test]
fn the_penalty_is_actually_paid_by_a_shipped_pack() {
    let result = run("haulage-001.yaml");

    assert_eq!(
        route(&result),
        vec!["DRIVE_DEPOT_TO_FORD", "DRIVE_FORD_TO_YARD"]
    );

    // Two actions of cost 1 each. Anything above 2 is penalty, and this is
    // the assertion that says so: the plan costs more than the distance it
    // drives. Every earlier soft constraint in this repository was avoided
    // for free, which reads identically to a penalty that is never charged.
    assert_eq!(
        cost(&result),
        Fixed::from_int(7).expect("in range"),
        "expected 1 + 5 + 1; a cost of 2 would mean the penalty was not charged"
    );
}

#[test]
fn the_cheapest_route_is_not_the_shortest_one() {
    // The north bridge is two units and the ford route is seven, so the
    // engine is declining a route that is a third of the price. It is
    // declining it because nobody has rated it, which is the only reason
    // available -- there is no cost, precondition or goal that separates
    // the two crossings.
    let result = run("haulage-001.yaml");
    let taken = route(&result);

    assert!(
        !taken.iter().any(|step| step.contains("NORTH_BRIDGE")),
        "an unrated bridge must be refused, got {taken:?}"
    );
    assert!(
        !taken.iter().any(|step| step.contains("HILL")),
        "the hills cost 9 against the ford's 7; taking them means the \
         penalty was overcharged or the ford was wrongly blocked, got {taken:?}"
    );
}

#[test]
fn one_observation_changes_the_route() {
    // The pair is the point. These two experiments differ in exactly one
    // property -- `north_bridge.rating_ok`, absent in the first and true in
    // the second. The map, the costs, the goal and the truck are identical.
    let unrated = run("haulage-001.yaml");
    let rated = run("haulage-002.yaml");

    assert_eq!(
        route(&rated),
        vec!["DRIVE_DEPOT_TO_NORTH_BRIDGE", "DRIVE_NORTH_BRIDGE_TO_YARD"]
    );
    assert_eq!(cost(&rated), Fixed::from_int(2).expect("in range"));

    assert_ne!(
        route(&unrated),
        route(&rated),
        "if these agree, `on_undecidable` did nothing and the pack proves \
         nothing it was written to prove"
    );

    // Different question, so a different decision hash. Worth asserting
    // because the two runs share a model and an engine, and the input is
    // the only one of the five that moved.
    assert_ne!(unrated.record.decision, rated.record.decision);
    assert_ne!(unrated.record.input, rated.record.input);
    assert_eq!(unrated.record.model, rated.record.model);
    assert_eq!(unrated.record.engine, rated.record.engine);
}

#[test]
fn both_readings_of_undecidable_are_exercised_by_one_run() {
    // C001 blocks on undecidable and C002 permits, and `haulage-001` puts
    // both in that state at once: neither crossing is rated. The route it
    // returns is only reachable if the two settings genuinely differ --
    // block alone strands the truck on the hills, permit alone lets it use
    // the cheaper bridge.
    let result = run("haulage-001.yaml");

    assert_eq!(
        route(&result),
        vec!["DRIVE_DEPOT_TO_FORD", "DRIVE_FORD_TO_YARD"],
        "this route is the only one that distinguishes block from permit"
    );
}
