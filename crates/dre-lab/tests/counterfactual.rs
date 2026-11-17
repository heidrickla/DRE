//! A counterfactual must be worth no more than the run it is made of.
//!
//! The whole design rests on one claim: asking "what if X were different"
//! is the same as authoring a second experiment with X different, and
//! running it. If that is true, a counterfactual inherits every guarantee
//! an ordinary run has — determinism, replayability, a proof — and adds no
//! new surface to keep honest.
//!
//! It is checkable rather than merely arguable, because this repository
//! already has the pair: `haulage-001` and `haulage-002` differ in exactly
//! `north_bridge.rating_ok`, and `haulage-002`'s decision hash is
//! committed. So perturbing `-001` into `-002` must reproduce that hash
//! byte for byte, and if it ever does not, the counterfactual is computing
//! something the engine would not.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use dre_core::id::Id;
use dre_lab::counterfactual::{compare, Comparison, Perturbation};
use dre_lab::runner;
use dre_state::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn experiment_path(name: &str) -> PathBuf {
    repo_root().join("experiments").join(name)
}

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn ask(name: &str, perturbations: &[Perturbation]) -> Comparison {
    let path = experiment_path(name);
    let experiment = runner::load_experiment(&path).expect("experiment loads");
    let pack_dir = runner::resolve_pack(&path, &experiment.pack);
    compare(&experiment, &pack_dir, perturbations).expect("counterfactual runs")
}

fn rating_known() -> Vec<Perturbation> {
    vec![Perturbation {
        subject: id("north_bridge.rating_ok"),
        value: Some(Value::Boolean(true)),
        may_introduce: false,
    }]
}

fn rating_forgotten() -> Vec<Perturbation> {
    vec![Perturbation {
        subject: id("north_bridge.rating_ok"),
        value: None,
        may_introduce: false,
    }]
}

#[test]
fn a_counterfactual_reproduces_the_experiment_it_describes() {
    // The load-bearing test. `haulage-001` with the bridge rated *is*
    // `haulage-002`, so it must produce `haulage-002`'s committed decision
    // hash -- not merely a similar answer, the same one.
    let asked = ask("haulage-001.yaml", &rating_known());

    let committed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("experiments/records")
                .join("haulage-002.json"),
        )
        .expect("record is readable"),
    )
    .expect("record parses");

    assert_eq!(
        asked.alternative.record.decision.to_string(),
        committed["decision"].as_str().expect("decision field"),
        "the counterfactual reached a different decision than the experiment \
         that asks the same question, so it is not running the same engine"
    );
    assert_eq!(
        asked.alternative.record.input.to_string(),
        committed["input"].as_str().expect("input field"),
        "the perturbed input differs from the hand-authored one"
    );
}

#[test]
fn the_question_runs_in_both_directions() {
    // Adding an observation and removing one are the same operation with
    // the sign flipped, and the second is the one that matters for
    // deciding what is worth measuring.
    let added = ask("haulage-001.yaml", &rating_known());
    let removed = ask("haulage-002.yaml", &rating_forgotten());

    assert!(added.decision_changed());
    assert!(removed.decision_changed());

    // -001 perturbed forward and -002 perturbed backward describe the same
    // two worlds, so the pair of decisions must be the same pair.
    assert_eq!(
        added.base.record.decision,
        removed.alternative.record.decision
    );
    assert_eq!(
        added.alternative.record.decision,
        removed.base.record.decision
    );
}

#[test]
fn the_model_and_engine_are_held_still() {
    // What makes the comparison attributable. Both halves share a pack and
    // a build, so any movement in the decision belongs to the perturbation
    // and to nothing else.
    let asked = ask("haulage-001.yaml", &rating_known());

    assert_eq!(asked.base.record.model, asked.alternative.record.model);
    assert_eq!(asked.base.record.engine, asked.alternative.record.engine);
    assert_ne!(asked.base.record.input, asked.alternative.record.input);
}

#[test]
fn a_perturbation_that_changes_nothing_is_reported_as_such() {
    // Setting a property to the value it already holds produces an
    // identical input. "No change" then describes the question, not the
    // engine, and conflating the two would turn a typo into a finding.
    let asked = ask("haulage-002.yaml", &rating_known());

    assert!(!asked.input_changed());
    assert!(!asked.decision_changed());
}

#[test]
fn the_reported_deltas_name_what_actually_moved() {
    let asked = ask("haulage-001.yaml", &rating_known());

    let subjects: Vec<&str> = asked
        .deltas
        .iter()
        .map(|delta| delta.subject.as_str())
        .collect();

    assert_eq!(
        subjects,
        vec!["north_bridge.rating_ok"],
        "only the perturbed property should differ; this pack concludes \
         nothing else about the bridge"
    );

    // The plan moved even though no *concluded* property did, which is why
    // `decision_changed` reads the DecisionHash rather than diffing state:
    // the hash covers the plan and a state diff does not.
    let base = asked.base.plan.as_ref().expect("a base plan");
    let alternative = asked
        .alternative
        .plan
        .as_ref()
        .expect("an alternative plan");
    assert_ne!(base.actions, alternative.actions);
    assert!(asked.decision_changed());
}

#[test]
fn a_perturbation_naming_nothing_is_refused() {
    let path = experiment_path("haulage-001.yaml");
    let experiment = runner::load_experiment(&path).expect("loads");
    let pack_dir = runner::resolve_pack(&path, &experiment.pack);

    // A misspelling must not perturb nothing and report "no change". That
    // is a wrong answer wearing the shape of a real negative result, and it
    // is the same failure `deny_unknown_fields` exists to prevent in packs.
    let typo = vec![Perturbation {
        subject: id("north_bridge.rating_okay"),
        value: Some(Value::Boolean(true)),
        may_introduce: false,
    }];
    assert!(compare(&experiment, &pack_dir, &typo).is_err());

    let absent = vec![Perturbation {
        subject: id("no_such_entity.rating_ok"),
        value: Some(Value::Boolean(true)),
        may_introduce: false,
    }];
    assert!(compare(&experiment, &pack_dir, &absent).is_err());

    let malformed = vec![Perturbation {
        subject: id("nodots"),
        value: Some(Value::Boolean(true)),
        may_introduce: false,
    }];
    assert!(compare(&experiment, &pack_dir, &malformed).is_err());
}

#[test]
fn asking_twice_gives_the_same_answer() {
    // A counterfactual is two deterministic runs, so it is deterministic.
    // Worth asserting rather than assuming: the comparison walks two
    // states and two fired-rule lists, and an unordered collection
    // anywhere in that path would make the report vary between runs while
    // both underlying runs stayed reproducible.
    let first = ask("haulage-001.yaml", &rating_known());
    let second = ask("haulage-001.yaml", &rating_known());

    assert_eq!(
        first.alternative.record.decision,
        second.alternative.record.decision
    );
    assert_eq!(
        first.alternative.record.proof,
        second.alternative.record.proof
    );

    let names = |comparison: &Comparison| -> Vec<String> {
        comparison
            .deltas
            .iter()
            .map(|delta| delta.subject.clone())
            .collect()
    };
    assert_eq!(names(&first), names(&second));
    assert_eq!(first.newly_fired, second.newly_fired);
    assert_eq!(first.no_longer_fired, second.no_longer_fired);
}

/// The pack directory resolves the same way `dre run` resolves it.
#[test]
fn the_pack_resolves_relative_to_the_experiment() {
    let path = experiment_path("haulage-001.yaml");
    let experiment = runner::load_experiment(&path).expect("loads");
    let resolved = runner::resolve_pack(&path, &experiment.pack);
    assert!(
        Path::new(&resolved).is_dir(),
        "{resolved:?} is not a directory"
    );
}
