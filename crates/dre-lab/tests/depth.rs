//! The claim that the substrate is powerful, not merely trustworthy.
//!
//! Determinism was proven to an unusual standard long before anything hard
//! had been reasoned about. Outside the grid packs, the deepest inference
//! run in this repository was `cold-chain-release-002` at 12 steps, 8 rules
//! fired and 74 proof nodes — a conclusion a careful reader reaches by hand
//! in a couple of minutes, which is exactly what makes it a good
//! correctness test and no test of depth at all.
//!
//! `fault-network-001` composes contested evidence, arithmetic features,
//! 451 chained rules and a geometric space over sixty entities. Nobody
//! traces it.
//!
//! # Why a conclusion nobody traces can still be checked
//!
//! Four independent accountings, none of which requires following the
//! run, and each of which fails differently:
//!
//! 1. **The planted fault is recovered, uniquely.** The generator put the
//!    fault at `comp-2-4`; exactly one component may come back blamed.
//! 2. **The impaired set is the downstream closure, and its size is
//!    derivable by hand** from the construction formula: 1 + 3 + 5 + 7.
//!    Propagation reaching too far or not far enough moves this and
//!    nothing else.
//! 3. **The unknown-resolution count is exact.** 240 conclusions are
//!    declared unknown at the start — four per component — and precisely
//!    181 may be resolved: every `locally_faulty`, every `impaired`, every
//!    `severity`, and one single `is_root_cause`.
//! 4. **Every entity was classified, not just one.** Sixty points read
//!    into a one-dimensional space.
//!
//! The third is the one that catches the failure the others miss. An
//! engine that blamed every impaired component would satisfy (1) on the
//! planted component and (2) exactly, and resolve 196 unknowns rather than
//! 181.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use dre_lab::runner;
use dre_state::EpistemicStatus;

/// The experiment under test, and the component its generator planted.
const EXPERIMENT: &str = "fault-network-001.yaml";
const PLANTED: &str = "comp-2-4";

/// 1 planted + 3 in layer 3 + 5 in layer 4 + 7 in layer 5.
///
/// Worked out from `upstream()`'s formula rather than by running anything:
/// a component at index `w` depends on `(w + k) % width` for `k < degree`,
/// so the components depending on index 4 are 4, 3 and 2, and the closure
/// widens by two per layer until it wraps.
const IMPAIRED: usize = 16;

/// Four unknowns per component, sixty components.
const DECLARED_UNKNOWN: u64 = 240;

/// 60 `locally_faulty` + 60 `impaired` + 60 `severity` + one `is_root_cause`.
const RESOLVABLE: u64 = 181;

/// Sixty components, one geometric dimension each.
///
/// The number that says the space classified every instance rather than
/// one. Before a space named an entity *type* this could only ever have
/// been 1, whatever the pack declared.
const ACTIVE_DIMENSIONS: u64 = 60;

fn run() -> runner::RunResult {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("experiments")
        .join(EXPERIMENT);
    runner::run(&path).unwrap_or_else(|error| panic!("{EXPERIMENT} must run: {error}"))
}

/// Components whose `property` is a known or inferred `true`.
fn holding(result: &runner::RunResult, property: &str) -> Vec<String> {
    let mut found: Vec<String> = result
        .state
        .entities
        .iter()
        .filter(|(_, entity)| {
            entity.properties.iter().any(|(id, prop)| {
                id.as_str() == property
                    && matches!(
                        prop.status,
                        EpistemicStatus::Known | EpistemicStatus::Inferred
                    )
                    && prop
                        .value
                        .as_ref()
                        .is_some_and(|v| matches!(v, dre_state::Value::Boolean(true)))
            })
        })
        .map(|(id, _)| id.to_string())
        .collect();
    found.sort();
    found
}

#[test]
fn the_planted_fault_is_recovered_and_it_is_the_only_one() {
    let result = run();
    let blamed = holding(&result, "is_root_cause");

    // Uniqueness is the assertion, not membership. A pack that blamed
    // every impaired component would contain the right answer and be
    // useless, and `assert!(blamed.contains(...))` would not notice.
    assert_eq!(
        blamed,
        vec![PLANTED.to_string()],
        "expected exactly one root cause, the planted one"
    );
}

#[test]
fn impairment_spreads_exactly_as_far_as_the_construction_says() {
    let result = run();
    let impaired = holding(&result, "impaired");

    assert_eq!(
        impaired.len(),
        IMPAIRED,
        "impairment reached {} components, not {IMPAIRED}: {impaired:?}",
        impaired.len()
    );

    // The planted component is impaired by its own fault rather than by
    // inheritance, and nothing above it may be touched — layers 0 and 1
    // are strictly upstream, so an impaired component there means
    // propagation is running the wrong way down the edges.
    assert!(impaired.contains(&PLANTED.to_string()));
    assert!(
        !impaired
            .iter()
            .any(|c| c.starts_with("comp-0-") || c.starts_with("comp-1-")),
        "something upstream of the fault was impaired: {impaired:?}"
    );
}

#[test]
fn exactly_the_resolvable_unknowns_were_resolved() {
    let result = run();
    let counters = result.counters;

    assert_eq!(
        counters.unknown_initial, DECLARED_UNKNOWN,
        "the experiment should declare four unknown conclusions per component"
    );

    // Not a floor. Resolving *more* than this means the engine decided
    // something it had no grounds to decide — blaming a component whose
    // upstream is broken is the specific way this goes wrong, and it
    // shows up here as 136 rather than as a wrong answer anywhere else.
    assert_eq!(
        counters.unknown_resolved, RESOLVABLE,
        "resolved {} of {DECLARED_UNKNOWN}, expected exactly {RESOLVABLE}",
        counters.unknown_resolved
    );
}

#[test]
fn the_reasoning_is_deeper_than_anything_that_can_be_checked_by_hand() {
    let result = run();
    let counters = result.counters;

    // A floor rather than an equality, deliberately. The exact figures
    // (313 steps, 1119 evaluations, 133 firings) are pinned by the replay
    // record, which is the right place for them: this assertion is about
    // the experiment still being *worth having*, and it should survive a
    // change that legitimately shifts the count.
    //
    // The bar is the previous deepest run in the repository —
    // `cold-chain-release-002`, at 12 steps and 74 proof nodes. Anything
    // near that means the pack stopped composing and this file is
    // measuring nothing.
    assert!(
        counters.reasoning_steps > 200,
        "only {} reasoning steps — the deepest hand-checkable experiment is 12",
        counters.reasoning_steps
    );
    assert!(
        counters.rules_evaluated > 1000,
        "only {} rule evaluations",
        counters.rules_evaluated
    );
    assert!(
        result.proof.nodes().len() > 1000,
        "only {} proof nodes",
        result.proof.nodes().len()
    );
}

#[test]
fn nothing_in_the_network_ended_undecidable() {
    let result = run();

    // Impairment and soundness are inferred by two rule families that must
    // stay mutually exclusive through the fixpoint. If a component were
    // ever cleared before an upstream became impaired, the two would merge
    // as CONFLICTING rather than as a correction — a real hazard here,
    // because `clear` and `propagate` write the same property.
    let broken: Vec<String> = result
        .state
        .entities
        .iter()
        .flat_map(|(id, entity)| {
            entity
                .properties
                .iter()
                .filter(|(_, value)| {
                    matches!(
                        value.status,
                        EpistemicStatus::Conflicting | EpistemicStatus::Invalid
                    )
                })
                .map(move |(prop, value)| format!("{id}.{prop} = {:?}", value.status))
        })
        .collect();

    assert!(
        broken.is_empty(),
        "properties ended undecidable: {broken:?}"
    );
}

#[test]
fn the_whole_composition_is_reproducible() {
    let mut decisions = BTreeSet::new();
    let mut proofs = BTreeSet::new();

    // Fewer iterations than the small experiments get, because this one
    // runs 450 rules to a fixpoint. The conformance suite is where
    // repetition at volume lives; this is here so a change that made the
    // *deep* case non-deterministic cannot pass this file.
    for _ in 0..64 {
        let result = run();
        decisions.insert(result.record.decision.to_string());
        proofs.insert(result.record.proof.to_string());
    }

    assert_eq!(decisions.len(), 1, "{} decision hashes", decisions.len());
    assert_eq!(proofs.len(), 1, "{} proof hashes", proofs.len());
}

#[test]
fn one_space_classified_every_entity_of_its_type() {
    let result = run();

    // The capability this pack was the reason for. A geometric space names
    // an entity *type*, so declaring it once classifies all sixty
    // components; the section was unusable by this domain until it did.
    //
    // `active_dimensions` is the honest witness. It accumulates per
    // subject, so 60 here means sixty points were read into one
    // one-dimensional space — and a space that classified a single entity
    // would report 1 while every other assertion in this file still
    // passed.
    assert_eq!(
        result.counters.active_dimensions, ACTIVE_DIMENSIONS,
        "read {} points into the space, expected one per component",
        result.counters.active_dimensions
    );

    let mut bands: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entity in result.state.entities.values() {
        for (id, property) in entity.properties.iter() {
            if id.as_str() == "severity" {
                if let Some(dre_state::Value::Symbol(band)) = property.value.as_ref() {
                    *bands.entry(band.to_string()).or_default() += 1;
                }
            }
        }
    }

    // The planted component reads 40% short and lands in FAILED; every
    // other component reads 2% short and lands in NOMINAL. Both bands are
    // fixed by what the generator planted, so neither count is a
    // measurement of the engine's opinion.
    assert_eq!(bands.get("FAILED").copied(), Some(1));
    assert_eq!(bands.get("NOMINAL").copied(), Some(59));
    assert_eq!(
        bands.get("DEGRADED"),
        None,
        "nothing reads into the middle band"
    );
}
