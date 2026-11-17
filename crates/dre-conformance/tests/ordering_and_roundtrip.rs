//! Two more attacks from plan section 21.
//!
//! Both target the same failure mode from different angles: something
//! incidental — the order a file happened to list its rules, or a trip
//! through a serializer — reaching the result.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use dre_conformance::Permuter;
use dre_core::canonical::Canonical;
use dre_core::context::ExecutionContext;
use dre_core::id::Id;
use dre_model::{load_dir, parse_section, SectionKind};
use dre_proof::Trace;
use dre_state::{EpistemicStatus, StateBuilder, Value};
use dre_symbolic::{forward_chain, Rule, RulesSection};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn pack_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("model-packs")
        .join(name)
}

fn diagnosis_rules() -> Vec<Rule> {
    let pack = load_dir(&pack_dir("diagnosis")).expect("diagnosis pack loads");
    parse_section::<RulesSection>(&pack, SectionKind::Rules)
        .expect("rules parse")
        .expect("pack declares rules")
        .rules
}

fn diagnosis_state() -> dre_state::CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    builder
        .entity(id("machine"), id("machine"))
        .expect("entity");
    builder
        .observe(&id("machine"), id("temperature"), Value::Integer(850))
        .expect("temperature");
    builder
        .observe(&id("machine"), id("vibration"), Value::Integer(810))
        .expect("vibration");
    builder
        .observe(&id("machine"), id("pressure"), Value::Integer(410))
        .expect("pressure");
    builder
        .declare(
            &id("machine"),
            id("internal_state"),
            EpistemicStatus::Unobservable,
        )
        .expect("internal_state");
    builder.build()
}

/// Run the diagnosis rules in a seed-determined order.
fn run_with_rule_order(seed: u64) -> (String, String, Vec<String>) {
    let mut rules = diagnosis_rules();
    Permuter::new(seed).shuffle(&mut rules);

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let outcome = forward_chain(
        &rules,
        diagnosis_state(),
        &mut context,
        &mut trace,
        &Default::default(),
    )
    .expect("converges");

    (
        outcome.state.canonical_hash().to_string(),
        trace.graph().proof_hash().to_string(),
        outcome.fired.iter().map(ToString::to_string).collect(),
    )
}

#[test]
fn randomised_rule_file_ordering_changes_nothing() {
    // The order a pack lists its rules in is an accident of editing. If it
    // reached the result, two packs that say the same thing would reason
    // differently -- and the firing order exists precisely so it cannot.
    let reference = run_with_rule_order(0);

    let mut states = BTreeSet::new();
    let mut proofs = BTreeSet::new();

    for seed in 0..500 {
        let (state, proof, fired) = run_with_rule_order(seed);
        states.insert(state);
        proofs.insert(proof);
        assert_eq!(
            fired, reference.2,
            "seed {seed} fired rules in a different order"
        );
    }

    assert_eq!(states.len(), 1, "rule order reached the state");
    assert_eq!(proofs.len(), 1, "rule order reached the proof");
}

#[test]
fn the_shuffle_actually_reorders_the_rules() {
    // Guards the test above from passing vacuously.
    let baseline: Vec<String> = diagnosis_rules()
        .iter()
        .map(|rule| rule.id.to_string())
        .collect();
    assert!(baseline.len() >= 3, "need enough rules to permute");

    let orders: BTreeSet<Vec<String>> = (0..64)
        .map(|seed| {
            let mut rules = diagnosis_rules();
            Permuter::new(seed).shuffle(&mut rules);
            rules.iter().map(|rule| rule.id.to_string()).collect()
        })
        .collect();

    assert!(
        orders.len() > 1,
        "the permuter never reordered the rules, so the test proves nothing"
    );
}

#[test]
fn state_survives_a_serialization_round_trip() {
    // A snapshot that does not restore to the same canonical bytes cannot
    // be replayed from, however faithful it looks.
    let original = diagnosis_state();
    let json = serde_json::to_string(&original).expect("serialises");
    let restored: dre_state::CanonicalState = serde_json::from_str(&json).expect("deserialises");

    assert_eq!(
        original.canonical_hash(),
        restored.canonical_hash(),
        "round trip changed the canonical encoding"
    );
    assert_eq!(original, restored);
}

#[test]
fn epistemic_status_survives_the_round_trip() {
    // The states that carry no value are the ones a lossy round trip would
    // flatten -- UNOBSERVABLE and UNKNOWN both look like "absent" if the
    // encoding is careless, and they mean different things.
    let original = diagnosis_state();
    let json = serde_json::to_string(&original).expect("serialises");
    let restored: dre_state::CanonicalState = serde_json::from_str(&json).expect("deserialises");

    let status_of = |state: &dre_state::CanonicalState| {
        state
            .entities
            .get(&id("machine"))
            .expect("machine")
            .properties
            .get(&id("internal_state"))
            .expect("internal_state")
            .status
    };

    assert_eq!(status_of(&original), EpistemicStatus::Unobservable);
    assert_eq!(status_of(&restored), EpistemicStatus::Unobservable);
}

#[test]
fn hashes_survive_a_round_trip_through_their_own_encoding() {
    let hash = diagnosis_state().canonical_hash();
    let json = serde_json::to_string(&hash).expect("serialises");
    let restored: dre_core::hash::Hash32 = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(hash, restored);

    // Stored records are read by people, so the encoding is hex rather
    // than a 32-element array.
    assert!(json.starts_with('"'), "expected a hex string, got {json}");
    assert_eq!(json.len(), 66, "64 hex characters plus quotes");
}
