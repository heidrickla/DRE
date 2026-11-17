//! Evidence resolution driving a real experiment.
//!
//! The pair `diagnosis-001` / `diagnosis-002` is the interesting part.
//! Both ask the same question; one is *told* the temperature, the other
//! works it out from nine contested readings. The hashes should therefore
//! agree on the answer and disagree on the reasoning — which is precisely
//! why plan section 17 keeps `DecisionHash` and `ProofHash` separate.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_lab::runner;

fn experiment(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("experiments")
        .join(name)
}

fn run(name: &str) -> runner::RunResult {
    runner::run(&experiment(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn independence_beats_vote_count() {
    let result = run("diagnosis-002.yaml");
    assert!(
        result.passed(),
        "diagnosis-002 failed:\n  {}",
        result.failures.join("\n  ")
    );

    // Three sensors said 400 and two said 850. The three share a bus, so
    // they are one independent group against two. Majority counting would
    // have picked 400 and diagnosed a healthy machine.
    let temperature = result
        .state
        .entities
        .iter()
        .find(|(id, _)| id.as_str() == "machine")
        .and_then(|(_, entity)| {
            entity
                .properties
                .iter()
                .find(|(id, _)| id.as_str() == "temperature")
                .map(|(_, property)| property)
        })
        .expect("machine.temperature");

    assert_eq!(temperature.value, Some(dre_state::Value::Integer(850)));
    assert_eq!(temperature.status, dre_state::EpistemicStatus::Known);
}

#[test]
fn same_answer_different_reasoning_is_visible_in_the_hashes() {
    let declared = run("diagnosis-001.yaml");
    let resolved = run("diagnosis-002.yaml");

    // Resolution lands on the same canonical state the other experiment
    // declares outright, so the engine is answering the same question and
    // gives the same answer.
    assert_eq!(
        declared.record.input, resolved.record.input,
        "resolved state must match the directly declared one"
    );
    assert_eq!(
        declared.record.decision, resolved.record.decision,
        "same question, same answer"
    );

    // But one was told and the other worked it out. A decision-only
    // comparison would call these runs identical; they are not, and the
    // proof hash is what says so.
    assert_ne!(
        declared.record.proof, resolved.record.proof,
        "weighing nine readings is not the same reasoning as being told"
    );
}

#[test]
fn the_proof_records_which_readings_were_overruled() {
    let result = run("diagnosis-002.yaml");

    let overruled: Vec<&str> = result
        .proof
        .nodes()
        .iter()
        .filter_map(|node| match &node.kind {
            dre_proof::NodeKind::Rejected { candidate, .. } => Some(candidate.as_str()),
            _ => None,
        })
        .collect();

    // The three bus readings were overruled and must be named, not
    // silently dropped -- "why not 400" is the question this result
    // invites.
    for reading in ["E001", "E002", "E003"] {
        assert!(
            overruled.contains(&reading),
            "{reading} was overruled but not recorded; proof has {overruled:?}"
        );
    }
}

#[test]
fn evidence_resolution_is_reproducible() {
    let first = run("diagnosis-002.yaml");
    let second = run("diagnosis-002.yaml");
    assert_eq!(first.record.decision, second.record.decision);
    assert_eq!(first.record.proof, second.record.proof);
}
