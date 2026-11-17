//! A condition must not change meaning while being parsed.
//!
//! This file exists because one did. `Condition` was `#[serde(untagged)]`,
//! and an untagged enum discards the error from every variant it tries.
//! `Compare` and `Status` differ only in that `Compare` carries an
//! `operand`, and serde ignores unknown fields by default — so a `Compare`
//! whose operand failed to parse fell through to `Status`, which accepted
//! the very same mapping and dropped the operand on the floor.
//!
//! What an author got back was not an error. It was a different condition:
//! a status test where they had written a comparison. It loaded, it
//! validated clean — there was no operand left for a validator to object
//! to — and every rule built on it was silently dead.
//!
//! The tests below are all one question asked from different directions:
//! does a condition that cannot be read honestly *say so*.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_model::yaml;
use dre_symbolic::{Condition, Operator};

fn parse(source: &str) -> Result<Condition, String> {
    yaml::from_str::<Condition>(source).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------
// The failure that motivated the change.
// ---------------------------------------------------------------------

#[test]
fn a_comparison_against_a_float_is_an_error_not_a_status_test() {
    // The exact shape from the digital-evidence pack.
    let result = parse(
        "
subject: artifact.custody_gap
operator: GT
operand: 0.0
",
    );

    let error = result.expect_err("a float operand must be rejected outright");
    assert!(
        !error.is_empty(),
        "the rejection must carry a reason an author can act on"
    );
}

#[test]
fn the_degraded_reading_is_no_longer_reachable() {
    // Stated as the property rather than the mechanism: whatever a
    // condition with an `operand` key parses to, it is never a `Status`.
    // If this ever fails, an operand has been dropped somewhere.
    for operand in ["0.0", "{fixed: \"0\"}", "5", "true", "some-symbol"] {
        let source = format!("subject: a.b\noperator: GT\noperand: {operand}");
        if let Ok(parsed) = parse(&source) {
            assert!(
                !matches!(parsed, Condition::Status { .. }),
                "operand {operand} was dropped and the condition became a status test"
            );
        }
    }
}

// ---------------------------------------------------------------------
// The forms that must still work.
// ---------------------------------------------------------------------

#[test]
fn a_well_formed_comparison_parses() {
    let parsed = parse("subject: machine.temperature\noperator: GT\noperand: 800")
        .expect("a plain comparison must parse");

    match parsed {
        Condition::Compare {
            subject, operator, ..
        } => {
            assert_eq!(subject.as_str(), "machine.temperature");
            assert_eq!(operator, Operator::Gt);
        }
        other => panic!("expected a comparison, got {other:?}"),
    }
}

#[test]
fn a_status_test_parses_without_an_operand() {
    let parsed =
        parse("subject: machine.vibration\noperator: KNOWN").expect("a status test must parse");

    assert!(matches!(parsed, Condition::Status { .. }), "{parsed:?}");
}

#[test]
fn the_combining_forms_still_parse() {
    let parsed = parse(
        "
any:
  - subject: a.b
    operator: LT
    operand: 100
  - subject: a.c
    operator: KNOWN
",
    )
    .expect("`any` must parse");

    let Condition::Any { any } = parsed else {
        panic!("expected `any`, got {parsed:?}")
    };
    assert_eq!(any.len(), 2);
    assert!(matches!(any[0], Condition::Compare { .. }));
    assert!(matches!(any[1], Condition::Status { .. }));
}

#[test]
fn nesting_survives() {
    let parsed = parse(
        "
not:
  all:
    - subject: a.b
      operator: EQ
      operand: true
",
    )
    .expect("a nested condition must parse");

    let Condition::Not { not } = parsed else {
        panic!("expected `not`, got {parsed:?}")
    };
    assert!(matches!(*not, Condition::All { .. }));
}

// ---------------------------------------------------------------------
// Mismatches between operator and operand.
// ---------------------------------------------------------------------

#[test]
fn a_comparison_without_an_operand_is_an_error() {
    // Under the old reading this was the *intended* fallthrough: drop to
    // `Status` and evaluate `GT` as a status operator, which is always
    // Unknown. A rule written this way could never fire.
    let error = parse("subject: a.b\noperator: GT").expect_err("a comparison needs an operand");
    assert!(
        error.contains("operand"),
        "the error must name what is missing, got: {error}"
    );
}

#[test]
fn a_status_operator_with_an_operand_is_an_error() {
    let error = parse("subject: a.b\noperator: KNOWN\noperand: 5")
        .expect_err("a status test takes no operand");
    assert!(
        error.contains("operand"),
        "the error must name the surplus operand, got: {error}"
    );
}

#[test]
fn a_misspelled_key_is_an_error() {
    // The quietest of the lot under the old reading: `operand` misspelled
    // meant the mapping no longer matched `Compare`, so it became a
    // `Status` test and the comparison vanished.
    let error = parse("subject: a.b\noperator: GT\noperrand: 5")
        .expect_err("an unknown key must be rejected");
    assert!(
        error.contains("operrand"),
        "the error must name the key, got: {error}"
    );
}

#[test]
fn a_condition_that_is_both_forms_is_an_error() {
    let error = parse("subject: a.b\noperator: KNOWN\nany: []")
        .expect_err("a condition is one form or the other");
    assert!(!error.is_empty());
}

#[test]
fn an_empty_condition_is_an_error() {
    let error = parse("description: does nothing").expect_err("an empty condition must be caught");
    assert!(!error.is_empty());
}

// ---------------------------------------------------------------------
// Round-trip.
// ---------------------------------------------------------------------

#[test]
fn serializing_and_reading_back_gives_the_same_condition() {
    // Serialization is still untagged, so a condition writes out as the
    // same plain mapping an author would have written. The hand-written
    // reader has to agree with it, or stored proofs and replay records
    // stop round-tripping.
    let original = parse(
        "
all:
  - subject: a.b
    operator: GTE
    operand: {fixed: \"1.5\"}
  - not:
      subject: a.c
      operator: UNKNOWN
",
    )
    .expect("parses");

    let written = serde_json::to_string(&original).expect("serializes");
    let read_back: Condition = serde_json::from_str(&written).expect("reads back");

    assert_eq!(original, read_back);
}
