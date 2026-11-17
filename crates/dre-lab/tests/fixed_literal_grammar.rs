//! Two literal grammars, and the error that explains which is which.
//!
//! A fixed-point value is written `{fixed: "44.0"}` in a **value** position
//! — a property's value, a rule's operand — because there the kind is
//! ambiguous: `44` could be an integer and `"44.0"` could be a symbol. In a
//! position already typed `Fixed`, such as a feature-expression literal, an
//! action's cost or a constraint's penalty, there is nothing to
//! disambiguate and the tag is not part of the grammar.
//!
//! The third independently authored pack generalised the tagged form into a
//! feature expression, which is the natural mistake to make from a brief
//! that showed the tag everywhere else and never showed a fixed literal
//! inside an expression. They recovered in under a minute because the
//! validator said what the position wanted.
//!
//! That recovery is the thing this file protects. The strict grammar is
//! defensible on its own, but it is only *tolerable* while the error
//! explains itself — so the message is pinned here, not just the rejection.
//! An error that regressed to serde's default "invalid type: map" would
//! leave the next author guessing, and the tests would still pass if they
//! only asserted failure.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_model::yaml;
use dre_symbolic::FeaturesSection;

fn feature_with(literal: &str) -> String {
    format!(
        "features:\n  - id: F001\n    subject: pump.margin\n    expression:\n      \
         subtract:\n        - subject: pump.reading\n        - literal: {literal}\n"
    )
}

#[test]
fn a_whole_number_literal_is_accepted() {
    let parsed = yaml::from_str::<FeaturesSection>(&feature_with("44"));
    assert!(parsed.is_ok(), "{:?}", parsed.err());
}

#[test]
fn a_decimal_string_literal_is_accepted() {
    // The spelling the brief now shows, and the one a non-whole constant
    // has to use: a bare 44.0 is a float, and floats are refused
    // workspace-wide.
    let parsed = yaml::from_str::<FeaturesSection>(&feature_with("\"44.0\""));
    assert!(parsed.is_ok(), "{:?}", parsed.err());
}

#[test]
fn a_bare_float_literal_is_still_refused() {
    // Guarding the reason the decimal string exists at all. If this ever
    // starts parsing, the fixed-point discipline has a hole in it and the
    // test above is no longer evidence of anything.
    let parsed = yaml::from_str::<FeaturesSection>(&feature_with("44.0"));
    assert!(
        parsed.is_err(),
        "a bare float must not parse as fixed-point"
    );
}

#[test]
fn the_tagged_form_is_refused_here_and_says_why() {
    let parsed = yaml::from_str::<FeaturesSection>(&feature_with("{fixed: \"44.0\"}"));
    let error = parsed
        .expect_err("the tagged form is not this grammar")
        .to_string();

    // Rejection alone is not enough. The author who hit this recovered
    // because the message told them which spelling the position wanted;
    // asserting only `is_err` would let that guidance rot away silently,
    // which is the failure mode this whole repository keeps finding.
    assert!(
        error.contains("already typed as fixed-point"),
        "the error should name the reason, got: {error}"
    );
    assert!(
        error.contains("44.0"),
        "the error should show the accepted spelling, got: {error}"
    );
    assert!(
        error.contains("value positions"),
        "the error should say where the tagged form does belong, got: {error}"
    );
}

#[test]
fn the_tagged_form_is_still_correct_in_a_value_position() {
    // The other half, and the reason the grammars differ. A rule operand is
    // a `Value`, where `44` is an integer and `"44.0"` would be a symbol, so
    // the tag is doing real work rather than decorating.
    let rule = "rules:\n  - id: R001\n    if:\n      - subject: pump.efficiency\n        \
                operator: LT\n        operand: {fixed: \"0.70\"}\n    infer:\n      \
                - subject: pump.degraded\n        value: true\n";
    let parsed = yaml::from_str::<dre_symbolic::RulesSection>(rule);
    assert!(parsed.is_ok(), "{:?}", parsed.err());
}
