//! Experiment 1 (plan section 25): the symbolic logic problem.
//!
//! ```text
//! A → B
//! B → C
//! A = TRUE
//!
//! Determine C.
//! ```
//!
//! Runs the real `model-packs/logic` pack through the whole pipeline —
//! load, parse, build state, forward chain — rather than hand-constructing
//! rules, so it exercises the pack format too.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_core::context::ExecutionContext;
use dre_core::id::Id;
use dre_model::{load_dir, parse_section, SectionKind};
use dre_proof::{ProofGraph, Trace};
use dre_state::{CanonicalState, EpistemicStatus, StateBuilder, Value};
use dre_symbolic::{forward_chain, Outcome, RulesSection};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn logic_pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("model-packs/logic")
}

fn load_rules() -> Vec<dre_symbolic::Rule> {
    let pack = load_dir(&logic_pack_dir()).expect("logic pack loads");
    let section: RulesSection = parse_section(&pack, SectionKind::Rules)
        .expect("rules parse")
        .expect("logic pack declares rules");
    section.rules
}

/// `a` is true; `b` and `c` are explicitly unknown rather than absent.
fn initial_state() -> CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    for name in ["a", "b", "c"] {
        builder.entity(id(name), id("proposition")).expect("entity");
    }
    builder
        .observe(&id("a"), id("holds"), Value::Boolean(true))
        .expect("observe a");
    builder
        .declare(&id("b"), id("holds"), EpistemicStatus::Unknown)
        .expect("declare b");
    builder
        .declare(&id("c"), id("holds"), EpistemicStatus::Unknown)
        .expect("declare c");
    builder.build()
}

fn run() -> (Outcome, ProofGraph, ExecutionContext) {
    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let outcome = forward_chain(
        &load_rules(),
        initial_state(),
        &mut context,
        &mut trace,
        &Default::default(),
    )
    .expect("forward chaining converges");
    (outcome, trace.into_graph(), context)
}

fn property(state: &CanonicalState, entity: &str, name: &str) -> dre_state::Property {
    state
        .entities
        .get(&id(entity))
        .expect("entity present")
        .properties
        .get(&id(name))
        .expect("property present")
        .clone()
}

#[test]
fn the_pack_declares_two_rules() {
    let rules = load_rules();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].id.as_str(), "R001");
    assert_eq!(rules[1].id.as_str(), "R002");
}

#[test]
fn c_is_concluded_true() {
    let (outcome, _, _) = run();
    let c = property(&outcome.state, "c", "holds");
    assert_eq!(c.value, Some(Value::Boolean(true)));
}

#[test]
fn c_is_inferred_not_known() {
    let (outcome, _, _) = run();
    let c = property(&outcome.state, "c", "holds");

    // The distinction the engine exists to preserve: `c` was derived, not
    // observed. Collapsing INFERRED into KNOWN would erase the provenance
    // that makes the conclusion auditable.
    assert_eq!(c.status, EpistemicStatus::Inferred);

    // Whereas `a` really was observed.
    assert_eq!(
        property(&outcome.state, "a", "holds").status,
        EpistemicStatus::Known
    );
}

#[test]
fn both_rules_fire_in_chain_order() {
    let (outcome, _, _) = run();
    let fired: Vec<&str> = outcome.fired.iter().map(Id::as_str).collect();

    // R001 must precede R002: R002's condition only becomes true after
    // R001 has inferred `b`.
    assert_eq!(fired, ["R001", "R002"]);
}

#[test]
fn the_proof_records_each_firing() {
    let (outcome, proof, _) = run();

    let mut evaluated = 0;
    let mut fired = 0;
    let mut inferences = 0;
    for node in proof.nodes() {
        match &node.kind {
            dre_proof::NodeKind::RuleEvaluated { fired: did, .. } => {
                evaluated += 1;
                if *did {
                    fired += 1;
                }
            }
            dre_proof::NodeKind::Inference { .. } => inferences += 1,
            _ => {}
        }
    }

    assert_eq!(fired, 2, "both rules should fire");
    assert_eq!(inferences, 2, "each firing asserts one value");

    // Both rules fire in the first pass: inferences apply immediately, so
    // R002 sees `b` as soon as R001 concludes it. The whole A -> B -> C
    // chain therefore resolves in one round, and the second round only
    // confirms the fixpoint.
    assert_eq!(
        evaluated, 2,
        "each rule evaluated once before being retired"
    );
    assert_eq!(outcome.rounds, 2, "one round to fire, one to confirm");
}

#[test]
fn non_firing_evaluations_are_recorded() {
    // A proof that recorded only successes could not answer "why did this
    // rule not apply", which is usually the question asked of a result
    // someone distrusts. Built in code rather than added to the pack, so
    // the pack stays the minimal statement of the logic domain.
    use dre_symbolic::{Condition, Inference, Operator, Rule};

    let unsatisfiable = Rule {
        id: id("R900"),
        conditions: vec![Condition::Compare {
            subject: id("b.holds"),
            operator: Operator::Eq,
            operand: Value::Boolean(false),
        }],
        inferences: vec![Inference {
            subject: id("c.holds"),
            value: Value::Boolean(false),
        }],
        priority: 50,
        description: "cannot hold: b is concluded true".to_owned(),
    };

    let mut rules = load_rules();
    rules.push(unsatisfiable);

    let mut context = ExecutionContext::with_defaults().expect("context");
    let mut trace = Trace::new();
    let outcome = forward_chain(
        &rules,
        initial_state(),
        &mut context,
        &mut trace,
        &Default::default(),
    )
    .expect("converges");

    assert!(
        !outcome.fired.iter().any(|rule| rule.as_str() == "R900"),
        "R900 must not fire"
    );

    let recorded_miss = trace.graph().nodes().iter().any(|node| {
        matches!(
            &node.kind,
            dre_proof::NodeKind::RuleEvaluated { rule, fired: false } if rule.as_str() == "R900"
        )
    });
    assert!(
        recorded_miss,
        "the proof must record that R900 was tried and failed"
    );
}

#[test]
fn counters_match_the_proof() {
    let (_, proof, context) = run();
    assert_eq!(context.counters().rules_fired, 2);
    assert_eq!(context.counters().proof_nodes as usize, proof.len());
}

#[test]
fn the_result_is_reproducible() {
    let (first, first_proof, _) = run();
    let (second, second_proof, _) = run();

    use dre_core::canonical::Canonical;
    assert_eq!(
        first.state.canonical_hash(),
        second.state.canonical_hash(),
        "same problem must give the same state"
    );
    assert_eq!(
        first_proof.proof_hash(),
        second_proof.proof_hash(),
        "same problem must give the same reasoning"
    );
}

/// Follow one provenance edge and describe where it landed.
fn single_input(proof: &ProofGraph, node: dre_proof::NodeRef) -> &dre_proof::NodeKind {
    let dre_proof::NodeRef(index) = node;
    let entry = &proof.nodes()[index as usize];
    assert_eq!(
        entry.inputs.len(),
        1,
        "expected exactly one dependency, got {:?}",
        entry.inputs
    );
    let dre_proof::NodeRef(next) = entry.inputs[0];
    &proof.nodes()[next as usize].kind
}

fn find_inference(proof: &ProofGraph, subject: &str) -> dre_proof::NodeRef {
    let index = proof
        .nodes()
        .iter()
        .position(|node| {
            matches!(&node.kind, dre_proof::NodeKind::Inference { subject: found }
                if found.as_str() == subject)
        })
        .unwrap_or_else(|| panic!("no inference recorded for {subject}"));
    dre_proof::NodeRef(index as u32)
}

fn find_firing(proof: &ProofGraph, rule_id: &str) -> dre_proof::NodeRef {
    let index = proof
        .nodes()
        .iter()
        .position(|node| {
            matches!(&node.kind, dre_proof::NodeKind::RuleEvaluated { rule, fired: true }
                if rule.as_str() == rule_id)
        })
        .unwrap_or_else(|| panic!("no firing recorded for {rule_id}"));
    dre_proof::NodeRef(index as u32)
}

fn inputs_of(proof: &ProofGraph, node: dre_proof::NodeRef) -> &[dre_proof::NodeRef] {
    let dre_proof::NodeRef(index) = node;
    &proof.nodes()[index as usize].inputs
}

#[test]
fn a_conclusion_traces_back_through_the_chain_that_produced_it() {
    // The whole point of `A -> B -> C`: asking why `c` holds must lead to
    // `b`, and asking why `b` holds must lead to `a`. Before the proof
    // carried edges, both questions stopped at "some rule fired", and the
    // chain was only recoverable by reading the rules by hand.
    let (_, proof, _) = run();

    let concluded = find_inference(&proof, "c.holds");

    let via = single_input(&proof, concluded);
    assert!(
        matches!(via, dre_proof::NodeKind::RuleEvaluated { rule, fired: true }
            if rule.as_str() == "R002"),
        "c.holds should rest on R002 firing, got {via:?}"
    );

    // And R002 in turn rests on the node that established b.holds -- the
    // inference, not the rule behind it. One edge per step, so the chain
    // stays walkable rather than collapsing into a summary.
    assert_eq!(
        inputs_of(&proof, find_firing(&proof, "R002")),
        [find_inference(&proof, "b.holds")],
        "R002 must depend on whatever established b.holds"
    );

    // R001 read only `a.holds`, which this test declares directly rather
    // than through the runner, so nothing established it. An absent edge
    // is the truthful encoding of that -- inventing one would assert a
    // dependency the run never had.
    assert!(
        inputs_of(&proof, find_firing(&proof, "R001")).is_empty(),
        "nothing in this harness established a.holds, so R001 rests on no recorded node"
    );
}

#[test]
fn an_explanation_carries_the_chain_and_leaves_out_the_rest() {
    let (_, proof, _) = run();
    let concluded = find_inference(&proof, "c.holds");

    let chain = dre_proof::explain(&proof, concluded).expect("the node is in the graph");

    for expected in ["R001", "R002", "b.holds", "c.holds"] {
        assert!(
            chain.contains(expected),
            "explanation should mention {expected}:\n{chain}"
        );
    }

    // An explanation that is just the whole trace explains nothing. The
    // graph here is small, so the check is that the chain is a strict
    // subset rather than a specific size.
    let full = dre_proof::render(&proof);
    assert!(
        chain.len() < full.len(),
        "explanation should be narrower than the full trace"
    );
}

#[test]
fn a_rule_that_did_not_fire_is_not_in_the_chain() {
    // A proof that showed every rule tried, as a dependency of the answer,
    // would be claiming the answer rested on rules that contributed nothing.
    let (_, proof, _) = run();
    let chain =
        dre_proof::explain(&proof, find_inference(&proof, "b.holds")).expect("node is present");

    assert!(
        !chain.contains("R002"),
        "b.holds was established before R002 was ever evaluated:\n{chain}"
    );
}
