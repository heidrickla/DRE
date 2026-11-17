//! Replay against a committed baseline.
//!
//! The distinction this covers: comparing two fresh runs proves the engine
//! agrees with itself *right now*. Comparing against a stored record
//! proves it still agrees with what it decided when the record was made,
//! which is the only version that catches drift.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::hash::Hash32;
use dre_core::id::Id;
use dre_lab::runner;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn experiment(name: &str) -> PathBuf {
    repo_root().join("experiments").join(name)
}

/// Every experiment on disk, not a list of the ones remembered.
///
/// This was a hand-written array of twelve names until `grid-40-mid-001`
/// was added and the array did not notice. "Every experiment" has to mean
/// the directory, or the test asserts only that a list agrees with itself.
fn experiments() -> Vec<String> {
    let found = dre_lab::experiment_files_in(&repo_root().join("experiments"))
        .expect("experiments directory is readable");
    assert!(
        found.len() >= 13,
        "found {} experiments, which is fewer than were committed — the \
         enumeration is wrong, not the repository",
        found.len()
    );
    found
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(ToOwned::to_owned))
        .collect()
}

#[test]
fn every_experiment_has_a_committed_baseline() {
    for name in experiments() {
        let id = Id::new(name.trim_end_matches(".yaml")).expect("valid id");
        assert!(
            dre_replay::store::exists(&repo_root(), &id),
            "{name} has no stored record; run `dre run experiments/{name} --record`"
        );
    }
}

#[test]
fn every_experiment_reproduces_its_stored_baseline() {
    for name in experiments() {
        let (verdict, comparison) =
            runner::verify_against_stored(&experiment(&name)).expect("replay runs");
        assert!(
            verdict.is_match(),
            "{name} drifted from its baseline (recorded under engine {}): {verdict:?}",
            comparison.recorded_under
        );
    }
}

#[test]
fn a_tampered_baseline_is_detected() {
    // The failure this whole mechanism exists to catch. Rather than
    // corrupting a committed file, load the real record and verify a
    // deliberately wrong one against the fresh run.
    let name = "logic-001.yaml";
    let id = Id::new("logic-001").expect("valid id");
    let stored = dre_replay::store::load(&repo_root(), &id).expect("record loads");
    let fresh = runner::run(&experiment(name)).expect("runs");

    let mut tampered = stored.record.clone();
    tampered.decision = Hash32::of(b"a decision this engine never made");

    let verdict = dre_replay::verify(&tampered, &fresh.record);
    assert!(
        matches!(verdict, dre_replay::Verdict::DecisionMismatch { .. }),
        "a changed decision must be caught, got {verdict:?}"
    );
}

#[test]
fn a_changed_proof_alone_is_still_caught() {
    // Same decision, different reasoning. A decision-only comparison would
    // pass this, which is exactly why the record carries both.
    let id = Id::new("logic-001").expect("valid id");
    let stored = dre_replay::store::load(&repo_root(), &id).expect("record loads");
    let fresh = runner::run(&experiment("logic-001.yaml")).expect("runs");

    let mut tampered = stored.record.clone();
    tampered.proof = Hash32::of(b"reached the same answer another way");

    assert!(matches!(
        dre_replay::verify(&tampered, &fresh.record),
        dre_replay::Verdict::ProofMismatch { .. }
    ));
}

#[test]
fn a_record_from_a_different_problem_proves_nothing() {
    // Comparing across differing inputs must report NOT COMPARABLE rather
    // than a mismatch -- reporting failure there would blame the engine
    // for being asked a different question.
    let logic = dre_replay::store::load(&repo_root(), &Id::new("logic-001").expect("id"))
        .expect("record loads");
    let navigation = runner::run(&experiment("navigation-001.yaml")).expect("runs");

    assert!(matches!(
        dre_replay::verify(&logic.record, &navigation.record),
        dre_replay::Verdict::NotComparable { .. }
    ));
}

#[test]
fn stored_records_are_readable_hex() {
    // The reason these are committed JSON rather than a database: a
    // changed decision should be one obviously different line in review.
    let path = dre_replay::store::record_path(&repo_root(), &Id::new("logic-001").expect("id"));
    let text = std::fs::read_to_string(&path).expect("record readable");

    assert!(
        text.contains("\"decision\": \""),
        "decision should be a string"
    );
    assert!(
        text.contains("\"engine_version\""),
        "records name their engine"
    );
    assert!(
        !text.contains('['),
        "hashes should be hex strings, not byte arrays"
    );
}

#[test]
fn two_planning_problems_with_the_same_start_get_different_decisions() {
    // The regression this exists for. The planner works on a *clone* of
    // the state and returns a plan without applying it, so the state after
    // planning is the state before it -- and the DecisionHash, taken over
    // the state alone, captured no part of the plan.
    //
    // The consequence was not subtle once seen. navigation-001 solves a
    // 5-by-5 grid in eight moves; grid-40-001 solves a 40-by-40 grid in
    // seventy-eight, from a different pack with different obstacles. Both
    // start with an agent at the origin, so both used to hash to exactly
    // the same DecisionHash. Two different decisions, one hash.
    //
    // Replay still caught a changed route, because it compares the proof
    // too -- but it caught it as ProofMismatch, whose meaning is "same
    // decision, different reasoning". For a plan that is precisely
    // backwards.
    let small = runner::run(&experiment("navigation-001.yaml")).expect("runs");
    let large = runner::run(&experiment("grid-40-001.yaml")).expect("runs");

    assert_eq!(
        small.record.input, large.record.input,
        "the premise: both problems start from the same state"
    );
    assert_ne!(
        small.plan.as_ref().map(|plan| plan.actions.len()),
        large.plan.as_ref().map(|plan| plan.actions.len()),
        "the premise: the plans differ"
    );

    assert_ne!(
        small.record.decision, large.record.decision,
        "a different plan is a different decision"
    );
}

#[test]
fn the_decision_hash_moves_when_only_the_plan_moves() {
    // Sharper than the test above, which also differs by model. Here the
    // state is identical and the plan is the only thing that changed, so
    // nothing but the plan can be responsible for the hash moving.
    let result = runner::run(&experiment("navigation-001.yaml")).expect("runs");
    let plan = result.plan.clone().expect("navigation produces a plan");

    let mut longer = plan.clone();
    longer.actions.push(Id::new("MOVE_EAST").expect("valid id"));

    assert_ne!(
        runner::decision_hash_of(&result.state, Some(&plan)),
        runner::decision_hash_of(&result.state, Some(&longer)),
        "one extra step must change the decision"
    );
    assert_ne!(
        runner::decision_hash_of(&result.state, Some(&plan)),
        runner::decision_hash_of(&result.state, None),
        "planning nothing is not the same decision as planning something"
    );
}
