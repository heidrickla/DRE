//! The three MVP experiments (plan section 25).
//!
//! Runs each through the real CLI path — real packs, real experiment
//! files — so the suite fails if a pack, the loader, or the runner drifts.
//!
//! Iteration counts here are deliberately modest; `dre benchmark` is the
//! tool for the plan's 10,000-run figure. What these assert is that each
//! experiment produces the *right* answer and that repeating it produces
//! the *same* answer.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use dre_lab::runner;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn experiment(name: &str) -> PathBuf {
    repo_root().join("experiments").join(name)
}

fn run(name: &str) -> runner::RunResult {
    runner::run(&experiment(name)).unwrap_or_else(|error| panic!("{name} must run: {error}"))
}

/// Every declared expectation must hold, and the failure list is printed
/// in full rather than one at a time.
fn assert_passes(name: &str) {
    let result = run(name);
    assert!(
        result.passed(),
        "{name} failed its expectations:\n  {}",
        result.failures.join("\n  ")
    );
}

/// Repeating an experiment must give one decision and one proof.
fn assert_deterministic(name: &str, iterations: u32) {
    let mut decisions = BTreeSet::new();
    let mut proofs = BTreeSet::new();

    for _ in 0..iterations {
        let result = run(name);
        decisions.insert(result.record.decision.to_string());
        proofs.insert(result.record.proof.to_string());
    }

    assert_eq!(
        decisions.len(),
        1,
        "{name}: {} decision hashes",
        decisions.len()
    );
    assert_eq!(proofs.len(), 1, "{name}: {} proof hashes", proofs.len());
}

#[test]
fn experiment_1_logic() {
    assert_passes("logic-001.yaml");

    let result = run("logic-001.yaml");
    // C is concluded, and concluded as *derived* rather than observed.
    let c = result
        .state
        .entities
        .iter()
        .find(|(id, _)| id.as_str() == "c")
        .map(|(_, entity)| entity)
        .expect("entity c");
    let holds = c
        .properties
        .iter()
        .find(|(id, _)| id.as_str() == "holds")
        .map(|(_, property)| property)
        .expect("c.holds");

    assert_eq!(holds.value, Some(dre_state::Value::Boolean(true)));
    assert_eq!(holds.status, dre_state::EpistemicStatus::Inferred);
}

#[test]
fn experiment_2_diagnosis() {
    assert_passes("diagnosis-001.yaml");

    let result = run("diagnosis-001.yaml");

    // Three rules fire, not four. R010's condition asks whether an
    // UNOBSERVABLE property is KNOWN. A fourth firing would mean the
    // engine concluded something from its own blind spot.
    assert_eq!(result.counters.rules_fired, 3);
    assert!(!result.fired.iter().any(|rule| rule.as_str() == "R010"));
}

#[test]
fn experiment_3_navigation() {
    assert_passes("navigation-001.yaml");

    let result = run("navigation-001.yaml");
    let plan = result.plan.as_ref().expect("a plan must be produced");

    // Optimal length, and the wall genuinely forced a detour: a clear
    // grid would also admit an 8-step route, so length alone would not
    // show the obstacle was respected. The pinned route in the experiment
    // file is what asserts *which* optimum came back.
    assert_eq!(plan.actions.len(), 8);
}

/// Every experiment in the repository, not only the three the MVP shipped
/// with — and read from disk rather than listed, because a list is a claim
/// about coverage that nothing checks.
///
/// It was a hand-written array of twelve until `grid-40-mid-001` was added
/// and nothing objected. The suite stayed green: the array was still
/// consistent with itself, and the new experiment was simply outside the
/// only test that would have run it.
fn all_experiments() -> Vec<String> {
    let found = dre_lab::experiment_files_in(&repo_root().join("experiments"))
        .expect("experiments directory is readable");
    assert!(
        found.len() >= 13,
        "found {} experiments, fewer than were committed — this enumeration \
         is broken, and a broken one passes vacuously",
        found.len()
    );
    found
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(ToOwned::to_owned))
        .collect()
}

#[test]
fn every_experiment_passes_its_own_expectations() {
    // This checked three of twelve until 2026-08-15, and the other nine
    // included one that had been failing since the day it landed:
    // `cold-chain-release-001` asserted a property the plan would have
    // written, and the planner does not execute plans. `dre run` exited
    // non-zero on it every time, and nothing was watching.
    //
    // An experiment is a claim about what the engine concludes. One
    // nobody checks is a comment.
    for name in all_experiments() {
        assert_passes(&name);
    }
}

#[test]
fn all_three_are_reproducible() {
    for name in [
        "logic-001.yaml",
        "diagnosis-001.yaml",
        "navigation-001.yaml",
    ] {
        assert_deterministic(name, 50);
    }
}

#[test]
fn replay_reproduces_each_experiment() {
    for name in [
        "logic-001.yaml",
        "diagnosis-001.yaml",
        "navigation-001.yaml",
    ] {
        let (verdict, _) = runner::verify_replay(&experiment(name)).expect("replay runs");
        assert!(verdict.is_match(), "{name}: {verdict:?}");
    }
}

#[test]
fn the_three_domains_exercise_different_subsystems() {
    // Domain isolation is the claim that matters, and three variations of
    // one problem shape would not test it. Each pack must drive a
    // different part of the engine.
    let logic = run("logic-001.yaml");
    let diagnosis = run("diagnosis-001.yaml");
    let navigation = run("navigation-001.yaml");

    // Logic is pure inference: rules fire, nothing is searched.
    assert!(logic.counters.rules_fired > 0);
    assert_eq!(logic.counters.states_expanded, 0);
    assert!(logic.plan.is_none());

    // Diagnosis adds geometric classification, still without search.
    assert!(diagnosis.counters.rules_fired > 0);
    assert_eq!(diagnosis.counters.states_expanded, 0);
    let classified = diagnosis.state.entities.iter().any(|(_, entity)| {
        entity
            .properties
            .iter()
            .any(|(id, _)| id.as_str() == "nearest_region")
    });
    assert!(classified, "diagnosis must classify into a region");

    // Navigation is pure search: no rules at all.
    assert_eq!(navigation.counters.rules_fired, 0);
    assert!(navigation.counters.states_expanded > 0);
    assert!(navigation.plan.is_some());
}
