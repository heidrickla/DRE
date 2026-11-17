//! Pack validation, both directions.
//!
//! Two halves that are only worth anything together: every shipped pack
//! must validate clean, and a pack broken on purpose must be caught. A
//! validator that has never been shown failing is indistinguishable from
//! one that cannot fail, and the failures it looks for are all silent at
//! run time — a rule on a property that does not exist evaluates to
//! unknown, quietly never fires, and the answer looks perfectly ordinary.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_lab::validate::{self, Severity};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const PACKS: [&str; 7] = [
    "logic",
    "diagnosis",
    "navigation",
    "process",
    "grid-40",
    "cold-chain-release",
    "digital-evidence-triage",
];

/// Every experiment, with the pack it binds.
const BINDINGS: [(&str, &str); 12] = [
    ("logic-001.yaml", "logic"),
    ("diagnosis-001.yaml", "diagnosis"),
    ("diagnosis-002.yaml", "diagnosis"),
    ("diagnosis-003.yaml", "diagnosis"),
    ("navigation-001.yaml", "navigation"),
    ("process-001.yaml", "process"),
    ("process-002.yaml", "process"),
    ("grid-40-001.yaml", "grid-40"),
    ("cold-chain-release-001.yaml", "cold-chain-release"),
    ("cold-chain-release-002.yaml", "cold-chain-release"),
    ("digital-evidence-001.yaml", "digital-evidence-triage"),
    ("digital-evidence-002.yaml", "digital-evidence-triage"),
];

fn load(pack: &str) -> dre_model::ModelPack {
    dre_model::load_dir(&repo_root().join("model-packs").join(pack)).expect("pack loads")
}

fn messages(findings: &[validate::Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_shipped_pack_is_internally_consistent() {
    for pack in PACKS {
        let findings = validate::pack(&load(pack)).expect("validation runs");
        assert!(
            !validate::has_errors(&findings),
            "{pack} has validation errors:\n{}",
            messages(&findings)
        );
    }
}

#[test]
fn every_experiment_binds_its_pack_consistently() {
    for (experiment, pack) in BINDINGS {
        let given = dre_lab::load_experiment(&repo_root().join("experiments").join(experiment))
            .expect("experiment loads");
        let findings = validate::experiment(&load(pack), &given).expect("validation runs");
        assert!(
            !validate::has_errors(&findings),
            "{experiment} against {pack} has validation errors:\n{}",
            messages(&findings)
        );
    }
}

// ---------------------------------------------------------------------
// The negative control.
// ---------------------------------------------------------------------

fn broken_pack() -> dre_model::ModelPack {
    dre_model::load_dir(&fixtures().join("broken-pack")).expect("the fixture still loads cleanly")
}

#[test]
fn the_broken_fixture_still_parses() {
    // The premise of the whole exercise: these faults are not parse
    // errors. If the fixture stopped loading, the test below would be
    // proving something much less interesting.
    let pack = broken_pack();
    assert_eq!(pack.manifest.id.as_str(), "broken-fixture");
}

#[test]
fn a_typo_in_a_condition_is_caught() {
    // The quietest failure there is. `widget.temprature` evaluates to
    // Unknown, so R001 never fires and the run reports nothing unusual.
    let findings = validate::pack(&broken_pack()).expect("validation runs");
    let all = messages(&findings);

    assert!(
        all.contains("temprature"),
        "the typo must be reported:\n{all}"
    );
    assert!(
        all.contains("R001"),
        "the finding must name the rule, not just the property:\n{all}"
    );
}

#[test]
fn an_undeclared_conclusion_is_caught() {
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert!(
        all.contains("undeclared_conclusion") && all.contains("R002"),
        "an inferred property the ontology never declares must be reported:\n{all}"
    );
}

#[test]
fn geometry_faults_are_caught() {
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));

    assert!(
        all.contains("humidity") && all.contains("space"),
        "a dimension no entity type declares must be reported:\n{all}"
    );
    assert!(
        all.contains("nearest_region"),
        "classify_into writing an undeclared property must be reported:\n{all}"
    );
    assert!(
        all.contains("pressure") && all.contains("HOT"),
        "a region naming an axis the space does not have must be reported:\n{all}"
    );
}

#[test]
fn a_missing_centroid_coordinate_is_a_warning_not_an_error() {
    // COLD gives no coordinate for `humidity`. Distance ranking treats it
    // as zero, which may well be what the author meant -- so it is worth
    // saying and not worth failing over. Severities exist for this.
    let findings = validate::pack(&broken_pack()).expect("validation runs");

    let warning = findings
        .iter()
        .find(|finding| finding.location.contains("COLD"))
        .expect("COLD should produce a finding");

    assert_eq!(warning.severity, Severity::Warning);
    assert!(warning.message.contains("zero"), "{}", warning.message);
}

#[test]
fn the_fixture_produces_errors_at_all() {
    // Guards every test above from passing vacuously if validation
    // silently stopped running.
    let findings = validate::pack(&broken_pack()).expect("validation runs");
    assert!(
        validate::has_errors(&findings),
        "the deliberately broken fixture must produce errors"
    );
    assert!(
        findings.len() >= 7,
        "expected at least seven findings, got {}:\n{}",
        findings.len(),
        messages(&findings)
    );
}

#[test]
fn a_feature_writing_an_integer_declared_property_is_caught() {
    // Every feature produces fixed-point whatever the operation --
    // subtracting two integers yields Fixed, not Integer. A property
    // declared integer and written by a feature is the pack contradicting
    // itself, and the contradiction is silent: rules comparing it against
    // integer literals simply never fire.
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert!(
        all.contains("drift") && all.contains("holds fixed-point at run time"),
        "the declaration must be reported as contradicting the feature:\n{all}"
    );
}

#[test]
fn a_comparison_across_numeric_kinds_is_caught() {
    // The direct form of the same failure: a condition that can never be
    // decided, so the rule can never fire. Reported even when the ontology
    // is right and the rule is wrong.
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert!(
        all.contains("R004") && all.contains("never be decided"),
        "a cross-kind comparison must be reported:\n{all}"
    );
}

#[test]
fn findings_are_reported_in_a_stable_order() {
    // A validator whose output moved between runs would be an odd thing
    // for this project to ship.
    let first = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    let second = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert_eq!(first, second);
}

// ---------------------------------------------------------------------
// Faults that only a binding makes visible.
// ---------------------------------------------------------------------

fn broken_binding() -> Vec<validate::Finding> {
    let given = dre_lab::load_experiment(&fixtures().join("broken-experiment.yaml"))
        .expect("experiment loads");
    validate::experiment(&broken_pack(), &given).expect("validation runs")
}

#[test]
fn a_value_outside_its_declared_domain_is_caught() {
    // Exactly what EpistemicStatus::Invalid is defined for: observed, but
    // outside the declared domain of the property. Nothing produced that
    // status before, and nothing checked the domain either.
    // Bounds render through `Fixed`'s own Display, so an integer property
    // reads as `[0.000000, 100.000000]`. Deliberately not special-cased: a
    // second number format for the same type is how two formats drift.
    let all = messages(&broken_binding());
    assert!(
        all.contains("outside its declared domain") && all.contains("[0.000000, 100.000000]"),
        "500 on a property bounded [0, 100] must be reported:\n{all}"
    );
}

#[test]
fn a_property_on_the_wrong_type_is_reported_once_not_twice() {
    // A property that exists nowhere is the pack pass's finding; a real
    // property on the wrong type is the binding pass's. One fault, one
    // voice — otherwise a reader has to work out whether two lines are two
    // problems.
    let all = broken_binding();
    let about_typo: Vec<&validate::Finding> = all
        .iter()
        .filter(|finding| finding.message.contains("temprature"))
        .collect();

    assert_eq!(
        about_typo.len(),
        1,
        "the typo should be reported once, got:\n{}",
        messages(&all)
    );
}

#[test]
fn a_value_of_the_wrong_kind_is_caught() {
    let all = messages(&broken_binding());
    assert!(
        all.contains("grade") && all.contains("declared symbol"),
        "a boolean given to a symbol property must be reported:\n{all}"
    );
}

#[test]
fn an_undeclared_entity_type_is_caught() {
    let all = messages(&broken_binding());
    assert!(
        all.contains("gadget"),
        "an entity of a type the ontology does not define must be reported:\n{all}"
    );
}

#[test]
fn a_property_not_on_this_type_is_caught() {
    let all = messages(&broken_binding());
    assert!(
        all.contains("undeclared_on_this_type"),
        "setting a property the entity's type does not declare must be reported:\n{all}"
    );
}

#[test]
fn binding_finds_strictly_more_than_the_pack_alone() {
    // The layering claim, checked rather than asserted in a comment: the
    // stronger pass is a superset, so nothing is lost by running it.
    let alone = validate::pack(&broken_pack()).expect("validation runs");
    let bound = broken_binding();

    assert!(
        bound.len() > alone.len(),
        "binding should add findings: {} alone, {} bound",
        alone.len(),
        bound.len()
    );

    for finding in &alone {
        assert!(
            bound.contains(finding),
            "binding lost a finding the pack pass had: {finding}"
        );
    }
}

// ---------------------------------------------------------------------
// Symbol vocabularies.
// ---------------------------------------------------------------------

#[test]
fn a_symbol_outside_its_declared_vocabulary_is_caught() {
    // A typo in a *value* is as invisible at run time as a typo in a
    // subject. `degarded` compares equal to nothing, so every rule reading
    // it fails to fire and the run reports nothing unusual.
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert!(
        all.contains("degarded") && all.contains("healthy, degraded"),
        "the finding must name both the symbol and the vocabulary:\n{all}"
    );
}

#[test]
fn an_open_vocabulary_admits_anything() {
    // `grade` in the fixture declares no `values`, and R001 writes
    // `grade-b` to it. Absent must mean open, or every existing pack would
    // have to enumerate every symbol it ever writes.
    let all = messages(&validate::pack(&broken_pack()).expect("validation runs"));
    assert!(
        !all.contains("grade-b"),
        "a property with no declared vocabulary must not be policed:\n{all}"
    );
}

#[test]
fn a_region_the_vocabulary_does_not_admit_is_caught() {
    // The realistic version of the mistake: a region added later, in a
    // different file, with nothing connecting the two declarations.
    let pack = dre_model::load_dir(&fixtures().join("drifted-pack")).expect("fixture loads");
    let findings = validate::pack(&pack).expect("validation runs");

    assert!(
        validate::has_errors(&findings),
        "a region outside the declared vocabulary must be an error"
    );

    let all = messages(&findings);
    assert!(
        all.contains("WARM") && all.contains("HOT, COLD"),
        "the finding must name the region and the vocabulary:\n{all}"
    );

    // Exactly one finding: everything else about this pack is correct, and
    // a fixture that produced six would not show that this one is detected.
    assert_eq!(findings.len(), 1, "expected only the drift:\n{all}");
}

#[test]
fn every_shipped_pack_declares_its_regions_in_its_vocabulary() {
    // The cross-check applied to the real packs. `diagnosis` and `process`
    // both classify into a symbol property with a closed vocabulary, so
    // adding a region without extending it now fails here.
    for pack in PACKS {
        let findings = validate::pack(&load(pack)).expect("validation runs");
        assert!(
            !messages(&findings).contains("vocabulary"),
            "{pack} has a vocabulary finding:\n{}",
            messages(&findings)
        );
    }
}

/// Two exclusive booleans that nothing can falsify are reported.
///
/// The shape an outside author wrote without knowing it was a trap: a rule
/// asserts `..._high` above a threshold, another asserts `..._normal` below a
/// lower one, and neither is ever inferred false. Concluding one leaves the
/// other UNKNOWN, an unknown neither supports nor contradicts, and so a
/// measurement taken to rule an account *out* cannot do it. The author
/// predicted a normal reading would leave their storage hypothesis "actively
/// refuted"; the same account led before and after, in every regime of the
/// value they chose.
#[test]
fn exclusive_booleans_nothing_can_falsify_are_reported() {
    let pack = dre_model::load_dir(&repo_root().join("model-packs/checkout-layer-diagnosis"))
        .expect("pack loads");
    let findings = validate::pack(&pack).expect("validation runs");

    let reported: Vec<&validate::Finding> = findings
        .iter()
        .filter(|finding| finding.message.contains("cannot both hold"))
        .collect();

    assert_eq!(
        reported.len(),
        3,
        "all three measurement pairs in this pack have the defect, got: {reported:#?}"
    );
    for finding in &reported {
        assert_eq!(
            finding.severity,
            Severity::Warning,
            "the pack is self-consistent — this is suspicious, not broken"
        );
    }
    assert!(
        reported
            .iter()
            .any(|finding| finding.location.contains("R007")
                && finding.message.contains("datastore_write_latency_high")),
        "the pair an author's prediction actually broke on should be named"
    );
}

/// A pack that infers both polarities is not reported.
///
/// `opc-ua-access` writes `value: false` in ten places and models the
/// positive fact once, rather than a pair of one-way booleans. That is the
/// remedy the warning recommends, so it must not fire on it — a check that
/// flags the correct pattern alongside the broken one teaches nothing.
#[test]
fn a_pack_that_infers_both_polarities_is_not_reported() {
    for name in ["opc-ua-access", "opc-ua-access-precedence"] {
        let pack =
            dre_model::load_dir(&repo_root().join("model-packs").join(name)).expect("pack loads");
        let findings = validate::pack(&pack).expect("validation runs");
        assert!(
            !findings
                .iter()
                .any(|finding| finding.message.contains("cannot both hold")),
            "{name} models exclusivity correctly and must not be flagged"
        );
    }
}

/// The check is quiet on every other pack in the repository.
///
/// The general form of this defect — an observation nothing can make false —
/// holds of 85% of the boolean observations here, which would make it noise
/// rather than a finding. This tighter shape must stay rare enough to be
/// worth reading, and the only way to know is to count.
#[test]
fn the_exclusivity_check_is_quiet_on_every_other_pack() {
    let packs = std::fs::read_dir(repo_root().join("model-packs")).expect("model-packs");

    let mut noisy = Vec::new();
    for entry in packs.flatten() {
        let path = entry.path();
        if !path.is_dir()
            || path
                .file_name()
                .is_some_and(|name| name == "checkout-layer-diagnosis")
        {
            continue;
        }
        let Ok(pack) = dre_model::load_dir(&path) else {
            continue;
        };
        let Ok(findings) = validate::pack(&pack) else {
            continue;
        };
        let hits = findings
            .iter()
            .filter(|finding| finding.message.contains("cannot both hold"))
            .count();
        if hits > 0 {
            noisy.push(format!("{}: {hits}", path.display()));
        }
    }

    assert!(
        noisy.is_empty(),
        "the check should fire only where the defect is; also fired on: {noisy:?}"
    );
}

/// An account nothing can argue against is reported.
///
/// A hypothesis loses score only through contradiction, which needs an
/// observation it explains to be definitely *not* happening. Where every
/// observation an account rests on is one nothing can infer false, the
/// account can gain score and never lose it — and no measurement anywhere in
/// the pack will rule it out.
///
/// `checkout-layer-diagnosis` is the extreme case: **not one** of its four
/// accounts can be refuted. That is why an expert's chosen measurement, taken
/// specifically to rule an account out, could not do it.
#[test]
fn an_account_nothing_can_refute_is_reported() {
    let pack = dre_model::load_dir(&repo_root().join("model-packs/checkout-layer-diagnosis"))
        .expect("pack loads");
    let findings = validate::pack(&pack).expect("validation runs");

    let reported: Vec<&validate::Finding> = findings
        .iter()
        .filter(|finding| finding.message.contains("nothing can refute this account"))
        .collect();

    assert_eq!(
        reported.len(),
        4,
        "no account in this pack can be refuted, got: {reported:#?}"
    );
    for finding in &reported {
        assert_eq!(finding.severity, validate::Severity::Warning);
    }
}

/// The repaired variant is clean, and it took two passes to get there.
///
/// The first repair gave each *measurement* one property with both polarities
/// inferred. That silenced the pair-level warning entirely and made three of
/// the four accounts refutable — but `APP_NODE_GC_OR_LOCAL_STALL` survived,
/// because it rests entirely on `checkout_error_rate_high`, `api_p95_high`
/// and `app_pool_wait_high`, which are *symptoms* and were still one-way.
///
/// That is the case for having both checks. The pair-level warning was
/// satisfied by a repair that left an account no measurement could rule out,
/// and only the account-level check said so.
///
/// The second repair gave each symptom a rule concluding the negative at the
/// same threshold — exact complement, no undecided band, because a symptom
/// either exceeds its threshold or does not. Neither repair changes a single
/// answer on the frozen experiments: the pack becomes *able* to be wrong
/// without becoming differently right.
#[test]
fn the_repaired_variant_can_argue_against_every_account() {
    let pack =
        dre_model::load_dir(&repo_root().join("model-packs/checkout-layer-diagnosis-falsifiable"))
            .expect("pack loads");
    let findings = validate::pack(&pack).expect("validation runs");

    let complaints: Vec<&validate::Finding> = findings
        .iter()
        .filter(|finding| {
            finding.message.contains("nothing can refute this account")
                || finding.message.contains("cannot both hold")
        })
        .collect();

    assert!(
        complaints.is_empty(),
        "the fully repaired variant should raise neither warning, got: {complaints:#?}"
    );

    // And the pack it was derived from still raises all seven, so the two are
    // a live before-and-after rather than two packs that happen to differ.
    let original = dre_model::load_dir(&repo_root().join("model-packs/checkout-layer-diagnosis"))
        .expect("pack loads");
    let before = validate::pack(&original).expect("validation runs");
    assert_eq!(
        before
            .iter()
            .filter(|finding| {
                finding.message.contains("nothing can refute this account")
                    || finding.message.contains("cannot both hold")
            })
            .count(),
        7,
        "three exclusive pairs and four irrefutable accounts"
    );
}

/// The refutability check is quiet on every other pack.
///
/// The observation-level form of this question holds of 85% of the boolean
/// observations here, which would be noise. An account needs only *one*
/// refutable observation, and almost all have one, so the sharper question
/// stays rare — the only way to know is to count.
#[test]
fn the_refutability_check_is_quiet_on_every_other_pack() {
    let packs = std::fs::read_dir(repo_root().join("model-packs")).expect("model-packs");

    let mut noisy = Vec::new();
    for entry in packs.flatten() {
        let path = entry.path();
        let named = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if !path.is_dir() || named.starts_with("checkout-layer-diagnosis") {
            continue;
        }
        let Ok(pack) = dre_model::load_dir(&path) else {
            continue;
        };
        let Ok(findings) = validate::pack(&pack) else {
            continue;
        };
        let hits = findings
            .iter()
            .filter(|finding| finding.message.contains("nothing can refute this account"))
            .count();
        if hits > 0 {
            noisy.push(format!("{named}: {hits}"));
        }
    }

    assert!(
        noisy.is_empty(),
        "the check should stay rare enough to read; also fired on: {noisy:?}"
    );
}

/// A reference naming nothing was the one value kind nothing checked.
///
/// `reference` is documented as "reference to another entity", so a typo in
/// one produces a property that reads as a perfectly ordinary pointer at a
/// thing that is not there — and, before this, validated clean. Worse than
/// the same typo in a symbol, precisely because the name promises a target.
#[test]
fn a_reference_naming_no_entity_is_refused() {
    let dir = std::env::temp_dir().join("dre-dangling-reference");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp pack dir");

    std::fs::write(
        dir.join("model.yaml"),
        "model:
  id: ref-probe
  version: 0.1.0
  requires_engine: 0.20.0
",
    )
    .expect("model");
    std::fs::write(
        dir.join("ontology.yaml"),
        [
            "entities:",
            "  server:",
            "    properties:",
            "      primary_db:",
            "        type: reference",
            "  database:",
            "    properties:",
            "      healthy:",
            "        type: boolean",
        ]
        .join(
            "
",
        ),
    )
    .expect("ontology");

    // Built line by line: a `\` continuation in a Rust string strips the
    // leading whitespace on the next line, which is exactly the indentation
    // YAML needs.
    let experiment_for = |target: &str| {
        let pack_path = dir.display().to_string().replace(char::from(92), "/");
        [
            "experiment:".to_owned(),
            "  id: ref-probe".to_owned(),
            format!("  pack: {pack_path}"),
            "  given:".to_owned(),
            "    entities:".to_owned(),
            "      - id: web".to_owned(),
            "        type: server".to_owned(),
            "        properties:".to_owned(),
            "          - name: primary_db".to_owned(),
            format!("            value: {{ref: {target}}}"),
            "            status: KNOWN".to_owned(),
            "      - id: db-01".to_owned(),
            "        type: database".to_owned(),
            "        properties:".to_owned(),
            "          - name: healthy".to_owned(),
            "            value: false".to_owned(),
            "            status: KNOWN".to_owned(),
        ]
        .join(
            "
",
        )
    };

    let pack = dre_model::load_dir(&dir).expect("pack loads");

    let write_experiment = |name: &str, target: &str| {
        let path = dir.join(name);
        std::fs::write(&path, experiment_for(target)).expect("experiment");
        dre_lab::load_experiment(&path).expect("experiment loads")
    };

    let dangling = write_experiment("dangling.yaml", "db-does-not-exist");
    let findings = validate::experiment(&pack, &dangling).expect("validation runs");
    assert!(
        findings.iter().any(|finding| {
            finding.severity == validate::Severity::Error
                && finding.message.contains("db-does-not-exist")
                && finding.message.contains("references")
        }),
        "a dangling reference must be an error, got: {findings:#?}"
    );

    // And a reference that names something real is left alone, so the check
    // does not simply refuse the value kind.
    let sound = write_experiment("sound.yaml", "db-01");
    let findings = validate::experiment(&pack, &sound).expect("validation runs");
    assert!(
        !validate::has_errors(&findings),
        "a reference naming a declared entity is fine, got: {findings:#?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
