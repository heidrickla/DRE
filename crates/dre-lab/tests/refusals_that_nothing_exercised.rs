//! Refusals the documentation asserted and no test covered.
//!
//! Every one of these was claimed in prose before it was true, or claimed to
//! live somewhere it did not. That is the failure shape this repository keeps
//! finding — an assertion about engine behaviour that nothing exercises — and
//! it has now caught the same author twice in two days.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;

use dre_symbolic::Condition;

fn condition(yaml: &str) -> Result<Condition, serde_saphyr::Error> {
    dre_model::yaml::from_str::<Condition>(yaml)
}

/// A traversal alongside a combining form was **silently discarded**.
///
/// `all` was returned and the `reach` dropped, in a deserializer whose own
/// comment says everything structural about a reach is checked here rather
/// than in `dre validate`. A condition that quietly means less than it says
/// is the exact defect that cost an independently authored pack two rules and
/// three conclusions when `Condition` was an untagged enum.
#[test]
fn a_reach_beside_a_combiner_is_refused_not_dropped() {
    let parsed = condition(
        "all:\n  - subject: a.b\n    operator: KNOWN\nreach:\n  from: unit\n  \
         path:\n    - edge: mounts\n  read: available\ncollapse: some\noperator: KNOWN\n",
    );
    let error = parsed
        .expect_err("a reach beside `all` is not a condition")
        .to_string();
    assert!(
        error.contains("not more than one"),
        "the error should say why, got: {error}"
    );
}

/// `collapse` has no default and cannot be omitted — the whole design.
#[test]
fn a_reach_without_a_collapse_is_refused() {
    let parsed = condition(
        "reach:\n  from: unit\n  path:\n    - edge: mounts\n  read: available\noperator: KNOWN\n",
    );
    let error = parsed.expect_err("collapse is mandatory").to_string();
    assert!(error.contains("collapse"), "got: {error}");
}

/// "At least none of them" is true of everything.
#[test]
fn at_least_zero_is_refused() {
    let parsed = condition(
        "reach:\n  from: unit\n  path:\n    - edge: mounts\n  read: available\n\
         collapse:\n  at_least: 0\noperator: KNOWN\n",
    );
    assert!(parsed.is_err(), "at_least: 0 holds of everything");
}

/// A traversal in a goal is refused **before the run does anything**.
///
/// The claim was that this happened at load. It did not: the check lived
/// inside `search_plan`, which returns early when a pack declares no
/// `causal.yaml`, and only ever looked at `goals.first()`. A pack with goals
/// and no actions was never checked, and a second goal was never checked at
/// all — so the refusal the documentation promised did not exist for either.
#[test]
fn a_reach_in_a_goal_is_refused_even_with_no_actions_declared() {
    let dir = std::env::temp_dir().join("dre-reach-goal-refusal");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp pack dir");

    std::fs::write(
        dir.join("model.yaml"),
        "model:\n  id: reach-goal-probe\n  version: 0.1.0\n  requires_engine: 0.20.0\n",
    )
    .expect("model");
    std::fs::write(
        dir.join("ontology.yaml"),
        "entities:\n  unit:\n    properties:\n      ready:\n        type: boolean\n  \
         mount:\n    properties:\n      available:\n        type: boolean\n\
         relationships:\n  mounts:\n    from: unit\n    to: mount\n",
    )
    .expect("ontology");
    // Deliberately NO causal.yaml: this is the case the old check skipped.
    std::fs::write(
        dir.join("goals.yaml"),
        "goals:\n  - id: G001\n    conditions:\n      - reach:\n          from: unit\n          \
         path:\n            - edge: mounts\n          read: available\n        collapse: every\n        \
         operator: EQ\n        operand: true\n",
    )
    .expect("goals");

    let experiment = dir.join("probe.yaml");
    std::fs::write(
        &experiment,
        format!(
            "experiment:\n  id: reach-goal-probe\n  pack: {}\n  given:\n    entities:\n      \
             - id: web\n        type: unit\n        properties:\n          - name: ready\n",
            dir.display().to_string().replace(char::from(92), "/")
        ),
    )
    .expect("experiment");

    let error = dre_lab::runner::run(&PathBuf::from(&experiment))
        .expect_err("a reach in a goal must be refused")
        .to_string();
    assert!(
        error.contains("G001") && error.contains("reach"),
        "the refusal should name the goal, got: {error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An observation naming a predicate the type does not declare is **stated and
/// excluded**, not silently accepted (engine 0.22.0).
///
/// The hole: `defect_in` reached the ontology through `?`, so a lookup miss
/// returned `None` — which that function spells "no defect". The reading was
/// admitted, resolved, and written by `StateBuilder::set`, which does not
/// consult the ontology either, so nothing objected and the engine reasoned
/// over a property the model never said could exist.
///
/// ⚠ ASSERTED ON THE PROOF, not on final state. A state assertion passes just
/// as well if the rejection stops being *recorded*, and a defect that is
/// excluded without a record is indistinguishable from a reading nobody took —
/// which is the property `dre-evidence` exists to preserve.
#[test]
fn an_undeclared_predicate_is_a_defective_reading() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("undeclared-predicate.yaml");
    let result = dre_lab::runner::run(&path).expect("the fixture runs");
    let proof = dre_proof::render(&result.proof);

    assert!(
        proof.contains("no_such_property is not declared on type machine"),
        "the proof must name the reading and say why it was refused, got:\n{proof}"
    );
    assert!(
        proof.contains("REJECTED") && proof.contains("E001"),
        "the refusal must be a REJECTED node naming the observation, got:\n{proof}"
    );

    // The sound reading on the same entity still resolves. Stating a defect
    // must not discard the remainder -- "attempt to resolve" is the third step
    // of the standing rule, and a gate that threw the run away would pass a
    // rejection assertion while breaking the engine's actual contract.
    assert!(
        proof.contains("machine.temperature"),
        "the sound reading must still resolve, got:\n{proof}"
    );
}
