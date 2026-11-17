//! A key nothing reads must be an error, in every section.
//!
//! Serde's default is to skip fields it does not recognise. For a config
//! file that is a kindness; for a model pack it is the quietest failure
//! the format has. A misspelled `perconditions` does not produce an
//! unguarded action — it produces an action with *no* preconditions, which
//! is applicable everywhere and looks entirely ordinary in a plan. A
//! `hard: ture` is a soft constraint. A stray `weight` on a region is a
//! weighting the author believes is in effect and is not.
//!
//! None of these fail. They all narrow or widen the model silently, and
//! every downstream artefact — the decision, the proof, the replay record
//! — is internally consistent with the model that was actually loaded
//! rather than the one that was written.
//!
//! This is the same defect that cost an outside author two rules through
//! `Condition`, in the general case. `deny_unknown_fields` was already on
//! `RuntimeConfig`, `EvidencePolicy` and the pack manifest, so the
//! practice existed; it simply had not been applied to the sections a pack
//! author spends all their time in.
//!
//! Each test below is a pair. The valid document must parse — otherwise
//! the rejection proves nothing — and the same document with one stray key
//! must not.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_model::yaml;

/// Assert that `valid` parses as `T` and `invalid` does not.
fn pair<T: serde::de::DeserializeOwned>(label: &str, valid: &str, invalid: &str) {
    yaml::from_str::<T>(valid)
        .unwrap_or_else(|error| panic!("{label}: the valid document must parse: {error}"));

    let rejected = yaml::from_str::<T>(invalid);
    assert!(
        rejected.is_err(),
        "{label}: a key nothing reads was accepted and silently dropped"
    );
}

#[test]
fn a_stray_key_on_a_rule_is_rejected() {
    // `wehn` instead of `when`, which is itself not the key -- the point
    // being that neither spelling is `if`, and the old behaviour accepted
    // both by ignoring them, leaving a rule with no conditions that fires
    // unconditionally.
    pair::<dre_symbolic::RulesSection>(
        "rules",
        "rules:\n  - id: R001\n    if: []\n    infer: []\n    priority: 100\n",
        "rules:\n  - id: R001\n    if: []\n    infer: []\n    priority: 100\n    wehn: []\n",
    );
}

#[test]
fn a_stray_key_on_a_feature_is_rejected() {
    pair::<dre_symbolic::FeaturesSection>(
        "features",
        "features:\n  - id: F001\n    subject: a.b\n    expression: {literal: 1}\n",
        "features:\n  - id: F001\n    subject: a.b\n    expression: {literal: 1}\n    unit: celsius\n",
    );
}

#[test]
fn a_stray_key_on_an_action_is_rejected() {
    // The worst of them. A misspelled `preconditions` leaves an action
    // applicable in every state, and a plan built on it is valid, cheap,
    // and wrong.
    pair::<dre_causal::CausalSection>(
        "causal",
        "actions:\n  - id: ADVANCE\n    preconditions: []\n    effects: {}\n    cost: 1\n",
        "actions:\n  - id: ADVANCE\n    perconditions: []\n    effects: {}\n    cost: 1\n",
    );
}

#[test]
fn a_stray_key_on_a_constraint_is_rejected() {
    pair::<dre_planner::ConstraintsSection>(
        "constraints",
        "constraints:\n  - id: C001\n    conditions: []\n    hard: true\n",
        "constraints:\n  - id: C001\n    conditions: []\n    hard: true\n    severity: high\n",
    );
}

#[test]
fn a_misspelled_undecidable_policy_is_rejected() {
    // Specifically worth pinning: `on_undecidable` decides whether a
    // safety constraint blocks or permits when it cannot tell. Misspelled,
    // it silently took the default, and a pack that had deliberately asked
    // to permit would block every candidate instead.
    pair::<dre_planner::ConstraintsSection>(
        "constraints",
        "constraints:\n  - id: C001\n    conditions: []\n    hard: true\n    on_undecidable: permit\n",
        "constraints:\n  - id: C001\n    conditions: []\n    hard: true\n    on_undecideable: permit\n",
    );
}

#[test]
fn a_stray_key_on_a_goal_is_rejected() {
    pair::<dre_planner::GoalsSection>(
        "goals",
        "goals:\n  - id: ARRIVE\n    conditions: []\n",
        "goals:\n  - id: ARRIVE\n    conditions: []\n    priority: 1\n",
    );
}

#[test]
fn a_stray_key_on_a_region_is_rejected() {
    pair::<dre_geometry::GeometrySection>(
        "geometry",
        "space:\n  subject: machine\n  dimensions:\n    - id: temperature\n      weight: 1\nregions:\n  - id: HOT\n    centroid:\n      temperature: 900\n",
        "space:\n  subject: machine\n  dimensions:\n    - id: temperature\n      weight: 1\nregions:\n  - id: HOT\n    centroid:\n      temperature: 900\n    weight: 2\n",
    );
}

#[test]
fn a_stray_key_on_the_space_is_rejected() {
    // Where the geometry outputs are declared. A misspelled
    // `boundary_distance_into` is a property the author believes is being
    // written and is not, and the run reports nothing unusual.
    pair::<dre_geometry::GeometrySection>(
        "geometry",
        "space:\n  subject: machine\n  boundary_distance_into: margin\n  dimensions:\n    - id: temperature\n      weight: 1\n",
        "space:\n  subject: machine\n  boundary_distance_info: margin\n  dimensions:\n    - id: temperature\n      weight: 1\n",
    );
}

#[test]
fn a_stray_key_on_a_property_type_is_rejected() {
    // `values` is the closed vocabulary and `min`/`max` the domain. A
    // misspelling means the ontology declares no constraint at all, and
    // `dre validate` then has nothing to check against.
    pair::<dre_state::OntologySection>(
        "ontology",
        "entities:\n  machine:\n    properties:\n      grade:\n        type: symbol\n        values: [a, b]\n",
        "entities:\n  machine:\n    properties:\n      grade:\n        type: symbol\n        vaules: [a, b]\n",
    );
}

#[test]
fn a_stray_key_on_an_experiment_is_rejected() {
    pair::<dre_lab::ExperimentFile>(
        "experiment",
        "experiment:\n  id: test-001\n  pack: model-packs/logic\n  given:\n    entities: []\n",
        "experiment:\n  id: test-001\n  pack: model-packs/logic\n  given:\n    entities: []\n  packs: model-packs/other\n",
    );
}

#[test]
fn a_stray_key_on_an_expectation_is_rejected() {
    // An expectation is a claim. A misspelled key means the claim was
    // never made, and the experiment passes by not asserting anything.
    pair::<dre_lab::ExperimentFile>(
        "experiment",
        "experiment:\n  id: test-001\n  pack: model-packs/logic\n  given:\n    entities: []\n  expect:\n    plan_length: 1\n",
        "experiment:\n  id: test-001\n  pack: model-packs/logic\n  given:\n    entities: []\n  expect:\n    plan_lenght: 1\n",
    );
}

#[test]
fn every_shipped_pack_still_loads() {
    // The other half. A sweep like this is only safe if nothing shipped
    // was relying on a key being ignored -- and if something was, that is
    // a finding rather than a reason to relax the check.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for pack in [
        "logic",
        "diagnosis",
        "navigation",
        "process",
        "grid-40",
        "cold-chain-release",
        "digital-evidence-triage",
    ] {
        dre_model::load_dir(&root.join("model-packs").join(pack))
            .unwrap_or_else(|error| panic!("{pack} no longer loads: {error}"));
    }
}
