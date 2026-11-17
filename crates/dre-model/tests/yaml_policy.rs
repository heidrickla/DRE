//! Requirements the model pack YAML parser must satisfy.
//!
//! These pin the *behaviour* DRE depends on, not the crate providing it.
//! If the parser is ever swapped, this file is the contract the
//! replacement has to meet — which is the point: `docs/decisions.md`
//! records why `serde-saphyr` was chosen, and this records what would
//! have to remain true of anything replacing it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_model::yaml;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Rule {
    id: String,
    priority: i64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Pack {
    rules: Vec<Rule>,
}

#[test]
fn deserializes_into_typed_structs() {
    let parsed: Pack = yaml::from_str(
        "
rules:
  - id: R001
    priority: 100
  - id: R002
    priority: 200
",
    )
    .expect("well-formed pack must parse");

    assert_eq!(parsed.rules.len(), 2);
    assert_eq!(parsed.rules[0].id, "R001");
    assert_eq!(parsed.rules[1].priority, 200);
}

#[test]
fn duplicate_keys_are_rejected() {
    // A pack with two rules under one key must not silently keep one of
    // them. Rule id is the final tie-break in the firing order, so a
    // silently dropped duplicate would change which rule fires with no
    // indication that anything was lost.
    let result: Result<Pack, _> = yaml::from_str(
        "
rules:
  - id: R001
    priority: 100
    priority: 200
",
    );

    assert!(result.is_err(), "duplicate key must be an error");
}

#[test]
fn merge_keys_are_rejected() {
    // `<<` would let a rule inherit conditions from an anchor elsewhere in
    // the file, so the rule would not state what it tests.
    let result: Result<Pack, _> = yaml::from_str(
        "
base: &base
  priority: 100

rules:
  - id: R001
    <<: *base
",
    );

    assert!(result.is_err(), "merge key must be an error");
}

#[test]
fn norwegian_symbols_stay_strings() {
    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Grades {
        grade: String,
        country: String,
    }

    // Under YAML 1.1 inference both of these become booleans. `NO` is a
    // country code, `ON` is a province; either is a plausible symbol value
    // in a model pack, and neither should become `false`/`true`.
    let parsed: Grades =
        yaml::from_str("grade: ON\ncountry: NO\n").expect("unquoted symbols must parse as strings");

    assert_eq!(parsed.grade, "ON");
    assert_eq!(parsed.country, "NO");
}

#[test]
fn booleans_still_work_when_actually_boolean() {
    #[derive(Debug, Deserialize)]
    struct Flags {
        enabled: bool,
        disabled: bool,
    }

    let parsed: Flags =
        yaml::from_str("enabled: true\ndisabled: false\n").expect("true/false must parse");

    assert!(parsed.enabled);
    assert!(!parsed.disabled);
}

#[test]
fn alias_bombs_are_bounded() {
    // The "billion laughs" shape: each level references the one below ten
    // times, so expansion is exponential in the number of levels. Six
    // levels is 10^6 leaves from six short lines. An unbounded parser in
    // front of a bounded engine is an unbounded engine.
    /// Six levels deep, matching the bomb's expansion depth.
    type Expanded = Vec<Vec<Vec<Vec<Vec<Vec<String>>>>>>;

    #[derive(Debug, Deserialize)]
    struct Bomb {
        #[allow(dead_code)]
        f: Expanded,
    }

    let bomb = "
a: &a [x, x, x, x, x, x, x, x, x, x]
b: &b [*a, *a, *a, *a, *a, *a, *a, *a, *a, *a]
c: &c [*b, *b, *b, *b, *b, *b, *b, *b, *b, *b]
d: &d [*c, *c, *c, *c, *c, *c, *c, *c, *c, *c]
e: &e [*d, *d, *d, *d, *d, *d, *d, *d, *d, *d]
f: [*e, *e, *e, *e, *e, *e, *e, *e, *e, *e]
";

    let result: Result<Bomb, _> = yaml::from_str(bomb);
    assert!(result.is_err(), "alias expansion must be bounded");
}

#[test]
fn malformed_input_returns_an_error_rather_than_panicking() {
    // Engine paths must not panic; that includes the parser they call.
    let result: Result<Pack, _> = yaml::from_str("rules: [unclosed\n  : : :\n");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------
// Floating point.
// ---------------------------------------------------------------------

/// A value in the position a rule operand occupies.
#[derive(Debug, Deserialize)]
struct Operand {
    operand: dre_state::Value,
}

#[test]
fn a_bare_float_is_rejected() {
    // `Value` refuses `f64` on purpose: an accepted float would be a hole
    // straight through the determinism guarantee, and its deserializer
    // says so in as many words.
    //
    // That refusal is only worth something if the parser actually hands
    // the scalar over *as a float*. A parser that offers `0.5` as
    // something else routes around the check without tripping it, and the
    // result is worse than a float would have been: the value loads, it
    // satisfies a fixed-point property's kind check, and it compares
    // equal to nothing. Every rule reading it is silently dead.
    //
    // This is the parser contract, not the deserializer's -- which is why
    // it lives here rather than beside `Value`.
    for source in ["operand: 0.5", "operand: 0.0", "operand: -2.25"] {
        let result: Result<Operand, _> = yaml::from_str(source);
        assert!(
            result.is_err(),
            "{source} must not parse; it did, as {:?}",
            result.map(|parsed| parsed.operand)
        );
    }
}

#[test]
fn the_rejection_names_the_form_that_works() {
    // A refusal an author cannot act on costs as much as it saves.
    let error = yaml::from_str::<Operand>("operand: 0.5")
        .expect_err("a bare float must be rejected")
        .to_string();

    assert!(
        error.contains("fixed"),
        "the error must point at the form that works, got: {error}"
    );
}

#[test]
fn the_quoted_fixed_form_still_parses() {
    // Guards the test above from being satisfied by rejecting everything.
    let parsed: Operand =
        yaml::from_str("operand: {fixed: \"0.5\"}").expect("the documented form must parse");

    assert_eq!(
        parsed.operand,
        dre_state::Value::Fixed(dre_core::fixed::Fixed::parse("0.5").expect("0.5 is fixed"))
    );
}
