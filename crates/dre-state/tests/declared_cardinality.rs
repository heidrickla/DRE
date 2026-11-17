//! Relationship cardinality, which nothing exercised.
//!
//! `docs/decisions.md` rests the safety of `collapse: the` on this and only
//! this: "`collapse: the` asserts a traversal is functional, and the only
//! thing that can make that assertion safe is the state refusing to hold a
//! second edge."
//!
//! `StateError::ArityViolated` was constructed in `builder.rs` and named
//! nowhere else in the repository — no test, and no shipped pack declares
//! `one` on either side. So the argument that makes the format's most
//! dangerous collapse safe had no coverage at all, which is the failure shape
//! this repository keeps finding and the reason
//! `dre-lab/tests/refusals_that_nothing_exercised.rs` exists.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use dre_core::collections::DetMap;
use dre_core::config::Limits;
use dre_core::id::Id;
use dre_state::ontology::{Cardinality, RelationshipType};
use dre_state::{StateBuilder, StateError};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

/// One `mounts` relationship, with the cardinality under test.
fn types(
    targets_per_source: Cardinality,
    sources_per_target: Cardinality,
) -> DetMap<Id, RelationshipType> {
    let mut declared = DetMap::new();
    declared.insert(
        id("mounts"),
        RelationshipType {
            from: id("unit"),
            to: id("mount"),
            targets_per_source,
            sources_per_target,
        },
    );
    declared
}

fn unit_and_two_mounts(types: DetMap<Id, RelationshipType>) -> StateBuilder {
    let mut builder = StateBuilder::new(Limits::default()).with_relationship_types(types);
    builder.entity(id("web"), id("unit")).expect("entity");
    builder.entity(id("api"), id("unit")).expect("entity");
    builder.entity(id("srv-data"), id("mount")).expect("entity");
    builder.entity(id("srv-logs"), id("mount")).expect("entity");
    builder
}

/// `targets_per_source: one` refuses the second edge out of one entity.
///
/// This is what makes `collapse: the` safe. Without it a pack may assert a
/// traversal is functional, and the state will happily hold the edge that
/// makes the assertion false.
#[test]
fn a_source_declared_to_have_one_target_refuses_a_second() {
    let mut builder = unit_and_two_mounts(types(Cardinality::One, Cardinality::Many));

    builder
        .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
        .expect("the first edge is fine");

    let error = builder
        .relate(id("M2"), id("mounts"), id("web"), id("srv-logs"))
        .expect_err("a second target from the same source must be refused");

    match error {
        StateError::ArityViolated {
            relationship_type,
            entity,
            side,
        } => {
            assert_eq!(relationship_type, id("mounts"));
            assert_eq!(entity, id("web"), "the refusal should name the source");
            assert_eq!(side, "targets_per_source: one");
        }
        other => panic!("expected ArityViolated, got {other:?}"),
    }
}

/// `sources_per_target: one` refuses the second edge into one entity.
///
/// The other half, and worth its own test because which side the "one" refers
/// to is precisely what `docs/decisions.md` says two explicit fields exist to
/// keep unambiguous.
#[test]
fn a_target_declared_to_have_one_source_refuses_a_second() {
    let mut builder = unit_and_two_mounts(types(Cardinality::Many, Cardinality::One));

    builder
        .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
        .expect("the first edge is fine");

    let error = builder
        .relate(id("M2"), id("mounts"), id("api"), id("srv-data"))
        .expect_err("a second source into the same target must be refused");

    match error {
        StateError::ArityViolated { entity, side, .. } => {
            assert_eq!(entity, id("srv-data"), "the refusal should name the target");
            assert_eq!(side, "sources_per_target: one");
        }
        other => panic!("expected ArityViolated, got {other:?}"),
    }
}

/// Declaring `one` on the side that is not being exceeded changes nothing.
///
/// A cardinality check that refused too much would be caught by no other
/// test here, and would look exactly like a working one from the outside:
/// packs would simply fail to build states nobody had tried to build.
#[test]
fn the_other_side_stays_many() {
    let mut builder = unit_and_two_mounts(types(Cardinality::One, Cardinality::Many));

    builder
        .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
        .expect("edge");
    builder
        .relate(id("M2"), id("mounts"), id("api"), id("srv-data"))
        .expect("two sources may share a target when only the other side is `one`");
}

/// Undeclared cardinality is `many`, so nothing an existing pack built moves.
///
/// Both fields default, and the weakest claim has to be the default: packs
/// that already declared relationships must not have gained a guarantee they
/// never asked for.
#[test]
fn cardinality_defaults_to_many() {
    let mut builder = unit_and_two_mounts(types(Cardinality::default(), Cardinality::default()));

    builder
        .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
        .expect("edge");
    builder
        .relate(id("M2"), id("mounts"), id("web"), id("srv-logs"))
        .expect("an undeclared relationship holds as many edges as it likes");
}

/// The claim `docs/decisions.md` actually makes, end to end.
///
/// `collapse: the` is safe *because* a state declaring the traversal
/// functional cannot hold the second edge. So the ambiguity `the` refuses at
/// evaluation time is unreachable when the cardinality is declared — the
/// state refuses first, and the pack learns about it while building rather
/// than while reasoning.
#[test]
fn declaring_one_makes_an_ambiguous_traversal_unbuildable() {
    let mut builder = unit_and_two_mounts(types(Cardinality::One, Cardinality::Many));
    builder
        .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
        .expect("edge");

    assert!(
        builder
            .relate(id("M2"), id("mounts"), id("web"), id("srv-logs"))
            .is_err(),
        "the state a `the` traversal would be ambiguous over cannot be built"
    );

    let state = builder.build();
    let reached = state
        .relationships
        .values()
        .filter(|edge| edge.type_id == id("mounts") && edge.from == id("web"))
        .count();
    assert_eq!(reached, 1, "so `the` has exactly one target to read");
}
