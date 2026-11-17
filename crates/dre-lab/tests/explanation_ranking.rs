//! The explanation layer, end to end, against the domain that demanded it.
//!
//! `device-cascade` is kept and `device-cascade-abduction` is added beside
//! it, so the pair is the claim: identical device, identical evidence, and
//! the only difference is whether the diagnosis comes from rules asserting
//! root causes or from a layer that ranks declared accounts.
//!
//! These tests are written against the committed experiments rather than
//! against hand-built fixtures on purpose. A fixture would prove the scoring
//! function computes what it computes; running the real packs proves the
//! engine reaches a different answer than it used to, which is the thing
//! actually being claimed.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::fixed::Fixed;
use dre_lab::runner;
use dre_state::EpistemicStatus;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn experiment(name: &str) -> PathBuf {
    repo_root().join("experiments").join(name)
}

fn root_cause(result: &runner::RunResult) -> (Option<String>, EpistemicStatus) {
    let entity = result
        .state
        .entities
        .iter()
        .find(|(id, _)| id.as_str() == "boiler-1")
        .expect("boiler-1 is in the state")
        .1;
    let property = entity
        .properties
        .iter()
        .find(|(id, _)| id.as_str() == "root_cause")
        .expect("root_cause is declared")
        .1;
    (
        property.value.as_ref().map(|value| match value {
            dre_state::Value::Symbol(symbol) => symbol.to_string(),
            other => format!("{other:?}"),
        }),
        property.status,
    )
}

/// The whole claim, in one comparison.
///
/// Two correct rules concluding two different root causes merge to
/// `CONFLICTING`, which is the right answer to "do my rules agree" and no
/// answer at all to "which account is better". The same device and the same
/// readings, with the diagnosis moved into a declared explanation layer,
/// reach `POWER`.
#[test]
fn rules_alone_conflict_where_a_ranking_decides() {
    let by_rules = runner::run(&experiment("device-cascade-002.yaml")).expect("runs");
    assert_eq!(
        root_cause(&by_rules),
        (None, EpistemicStatus::Conflicting),
        "device-cascade is the evidence that rules cannot rank; if this \
         stops conflicting, the pack has been changed and the finding lost"
    );

    let by_ranking = runner::run(&experiment("device-cascade-abduction-001.yaml")).expect("runs");
    assert_eq!(
        root_cause(&by_ranking),
        (Some("POWER".to_owned()), EpistemicStatus::Inferred)
    );
}

/// The number that separates this from counting symptoms.
///
/// `POWER` accounts for five observations. Four of them hang off one
/// mechanism, so they are one thing reported four times and score
/// `1 + 3 x 0.25` rather than `4`. A symptom count would have produced 5.00
/// of coverage, the same winner, and the wrong reason — which is precisely
/// the failure mode worth pinning, because the right answer for the wrong
/// reason is invisible until the case where it gives the wrong answer.
#[test]
fn coverage_discounts_correlated_symptoms() {
    let result = runner::run(&experiment("device-cascade-abduction-001.yaml")).expect("runs");
    let ranking = result.ranking.expect("the pack declares hypotheses");

    let power = &ranking.scored[0];
    assert_eq!(power.hypothesis.as_str(), "POWER");
    assert_eq!(power.coverage, Fixed::from_raw(2_750_000));
    assert_eq!(power.score, Fixed::from_raw(2_400_000));

    let thermal = &ranking.scored[1];
    assert_eq!(thermal.hypothesis.as_str(), "THERMAL");
    assert_eq!(thermal.score, Fixed::from_raw(900_000));

    assert_eq!(ranking.margin(), Some(Fixed::from_raw(1_500_000)));
}

/// The same pack, the other answer.
///
/// Without this the first two tests prove nothing: a pack whose ranking
/// always returned `POWER` would be indistinguishable from one with the
/// answer written into it. A healthy rail refutes `POWER`'s leading
/// mechanism where it is checkable, and the four correlated symptoms are no
/// longer enough to carry an account that has been contradicted.
#[test]
fn the_same_pack_reaches_the_other_answer_on_other_evidence() {
    let result = runner::run(&experiment("device-cascade-abduction-003.yaml")).expect("runs");
    assert_eq!(
        root_cause(&result),
        (Some("THERMAL".to_owned()), EpistemicStatus::Inferred)
    );

    let ranking = result.ranking.expect("the pack declares hypotheses");
    let power = ranking
        .scored
        .iter()
        .find(|scored| scored.hypothesis.as_str() == "POWER")
        .expect("POWER is still ranked");
    assert_eq!(power.contradicted.len(), 1);
}

/// A step nobody checked is charged for, and differently from a refuted one.
///
/// The failed temperature read leaves `overheating` undecided rather than
/// false, so `THERMAL`'s leading mechanism becomes an assumption. Under
/// rules this was invisible: the seductive rule simply did not fire, and
/// "the premise is unconfirmed" and "the premise is false" produced the
/// same silence.
#[test]
fn an_unchecked_step_is_recorded_as_an_assumption() {
    let result = runner::run(&experiment("device-cascade-abduction-002.yaml")).expect("runs");
    let ranking = result.ranking.expect("the pack declares hypotheses");

    let thermal = ranking
        .scored
        .iter()
        .find(|scored| scored.hypothesis.as_str() == "THERMAL")
        .expect("THERMAL is ranked");
    assert_eq!(thermal.assumptions, 1);
    assert!(thermal.contradicted.is_empty());
    assert_eq!(ranking.undecided.len(), 1);
}

/// Every hypothesis is in the proof, not only the winner.
///
/// "Why POWER and not THERMAL" is the question a diagnosis is judged on, and
/// a proof that recorded the decision without the comparison could not
/// answer it a week later. The losing hypothesis's score is part of the
/// audit trail.
#[test]
fn the_proof_records_the_comparison_and_not_only_the_verdict() {
    let result = runner::run(&experiment("device-cascade-abduction-001.yaml")).expect("runs");

    let explained: Vec<&dre_core::id::Id> = result
        .proof
        .nodes()
        .iter()
        .filter_map(|node| match &node.kind {
            dre_proof::NodeKind::Explanation { hypothesis, .. } => Some(hypothesis),
            _ => None,
        })
        .collect();
    assert_eq!(explained.len(), 2, "both accounts should be recorded");

    let decisions: Vec<&dre_core::id::Id> = result
        .proof
        .nodes()
        .iter()
        .filter_map(|node| match &node.kind {
            dre_proof::NodeKind::Decision { candidate } => Some(candidate),
            _ => None,
        })
        .collect();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].as_str(), "POWER");
}

/// The ranking is a run like any other, so it replays.
///
/// Worth asserting rather than assuming. The layer sorts, compares
/// fixed-point quantities and breaks ties, all of which are the sort of
/// thing that acquires an unstable ordering without anybody noticing.
#[test]
fn a_ranked_run_reproduces_itself() {
    let (verdict, _) =
        runner::verify_replay(&experiment("device-cascade-abduction-001.yaml")).expect("runs");
    assert!(verdict.is_match(), "{verdict:?}");
}

/// Deterministic experimental design: which measurement settles the
/// diagnosis, and which one cannot.
///
/// `device-cascade-abduction-004` reads half a cascade. Either unread
/// subsystem can overturn the answer — false refutes the bus mechanism
/// outright — while the door can be read either way without moving one
/// account relative to the other.
///
/// The door is the assertion worth having. It is a perfectly ordinary
/// unknown whose answer changes the state, so the older "can this change
/// the decision" ranking has nothing to say against it. Only a layer that
/// holds two accounts at once can report that measuring it cannot settle
/// which is right.
#[test]
fn the_survey_separates_measurements_that_settle_the_diagnosis() {
    let path = experiment("device-cascade-abduction-004.yaml");
    let loaded = runner::load_experiment(&path).expect("loads");
    let pack_dir = runner::resolve_pack(&path, &loaded.pack);
    let pack = dre_model::load_dir(&pack_dir).expect("pack loads");

    let survey = dre_lab::survey::survey(&loaded, &pack, &pack_dir).expect("surveys");

    assert_eq!(
        survey.base_leader.as_ref().map(dre_core::id::Id::as_str),
        Some("POWER")
    );
    assert_eq!(survey.base_lead, Some(Fixed::from_raw(500_000)));

    let entry = |name: &str| {
        survey
            .surveyed
            .iter()
            .find(|found| found.subject.as_str() == name)
            .unwrap_or_else(|| panic!("{name} should be surveyed"))
    };

    let flow = entry("boiler-1.flow_sensor_error");
    assert!(flow.flips_leader());
    assert_eq!(flow.discrimination(), Some(Fixed::from_raw(2_500_000)));

    let door = entry("boiler-1.door_open");
    assert!(!door.flips_leader());
    assert_eq!(
        door.discrimination(),
        Some(Fixed::ZERO),
        "either answer leaves both accounts exactly where they were"
    );

    // Discriminating measurements come first, so the report opens with the
    // trip worth taking.
    assert!(survey.surveyed[0].flips_leader());

    // And the property the ranking writes is not something to go and read
    // off an instrument. This was offered as measurable on the first run —
    // the same declared-but-unread shape as every other section that
    // arrived carrying a subject the survey did not know to exclude.
    assert!(
        !survey
            .surveyed
            .iter()
            .any(|found| found.subject.as_str() == "boiler-1.root_cause"),
        "root_cause is written by the explanation layer, not acquired"
    );
}
