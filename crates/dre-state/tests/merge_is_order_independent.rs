//! The state builder's merge rule must be commutative.
//!
//! Declaration order is an accident of how a model pack was written. If it
//! reaches the built state, it reaches `InputHash`, and the determinism
//! guarantee is gone. This exhaustively checks every ordering of small
//! fact sets rather than spot-checking a few.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_core::canonical::Canonical;
use dre_core::id::Id;
use dre_state::{EpistemicStatus, StateBuilder, Value};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

/// One fact to feed the builder.
#[derive(Clone, Copy, Debug)]
enum Fact {
    Observe(&'static str, i64),
    Infer(&'static str, i64),
}

fn apply(facts: &[Fact]) -> dre_state::CanonicalState {
    let mut builder = StateBuilder::with_default_limits();
    let subject = id("machine");
    builder
        .entity(subject.clone(), id("generic-object"))
        .expect("entity");

    for fact in facts {
        match *fact {
            Fact::Observe(property, value) => builder
                .observe(&subject, id(property), Value::Integer(value))
                .expect("observe"),
            Fact::Infer(property, value) => builder
                .infer(&subject, id(property), Value::Integer(value))
                .expect("infer"),
        };
    }

    builder.build()
}

/// Every permutation of `facts`, via Heap's algorithm.
fn permutations(facts: &[Fact]) -> Vec<Vec<Fact>> {
    fn generate(current: &mut Vec<Fact>, k: usize, out: &mut Vec<Vec<Fact>>) {
        if k <= 1 {
            out.push(current.clone());
            return;
        }
        for index in 0..k {
            generate(current, k - 1, out);
            if k % 2 == 0 {
                current.swap(index, k - 1);
            } else {
                current.swap(0, k - 1);
            }
        }
    }

    let mut current = facts.to_vec();
    let mut out = Vec::new();
    let len = current.len();
    generate(&mut current, len, &mut out);
    out
}

fn assert_order_independent(facts: &[Fact]) {
    let orderings = permutations(facts);
    assert!(orderings.len() > 1, "test needs more than one ordering");

    let reference = apply(&orderings[0]).canonical_hash();
    for ordering in &orderings {
        assert_eq!(
            apply(ordering).canonical_hash(),
            reference,
            "ordering {ordering:?} produced a different state"
        );
    }
}

fn status_of(state: &dre_state::CanonicalState, property: &str) -> EpistemicStatus {
    state
        .entities
        .get(&id("machine"))
        .expect("entity present")
        .properties
        .get(&id(property))
        .expect("property present")
        .status
}

#[test]
fn agreeing_observations_are_order_independent() {
    assert_order_independent(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 750),
        Fact::Observe("vibration", 830),
    ]);
}

#[test]
fn conflicting_observations_are_order_independent() {
    assert_order_independent(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 900),
        Fact::Observe("vibration", 830),
    ]);

    let state = apply(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 900),
    ]);
    assert_eq!(
        status_of(&state, "temperature"),
        EpistemicStatus::Conflicting
    );
}

#[test]
fn observation_beats_inference_either_way_round() {
    assert_order_independent(&[
        Fact::Observe("temperature", 750),
        Fact::Infer("temperature", 900),
    ]);

    // The asymmetric case: whichever arrives first, the observation wins.
    for facts in [
        [
            Fact::Observe("temperature", 750),
            Fact::Infer("temperature", 900),
        ],
        [
            Fact::Infer("temperature", 900),
            Fact::Observe("temperature", 750),
        ],
    ] {
        let state = apply(&facts);
        assert_eq!(status_of(&state, "temperature"), EpistemicStatus::Known);
    }
}

#[test]
fn mixed_channels_over_several_properties_are_order_independent() {
    assert_order_independent(&[
        Fact::Observe("temperature", 750),
        Fact::Infer("bearing_fault", 1),
        Fact::Observe("vibration", 830),
        Fact::Infer("temperature", 800),
    ]);
}

#[test]
fn conflict_is_absorbing() {
    // Once conflicting, a later agreeing observation must not "resolve" it
    // back to KNOWN -- that would make the result depend on arrival order.
    assert_order_independent(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 900),
        Fact::Observe("temperature", 750),
    ]);

    let state = apply(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 900),
        Fact::Observe("temperature", 750),
    ]);
    assert_eq!(
        status_of(&state, "temperature"),
        EpistemicStatus::Conflicting
    );
}

#[test]
fn conflicting_properties_carry_no_value() {
    let state = apply(&[
        Fact::Observe("temperature", 750),
        Fact::Observe("temperature", 900),
    ]);
    let property = state
        .entities
        .get(&id("machine"))
        .expect("entity")
        .properties
        .get(&id("temperature"))
        .expect("property");
    assert_eq!(property.value, None, "a conflict must not pick a winner");
}
