//! The `ontology.yaml` section of a model pack.
//!
//! Until this existed, every pack's ontology was decorative. It was loaded
//! as bytes — so it reached the `ModelHash`, and editing it changed the
//! model's identity — and then nothing read it. A pack could declare
//! `temperature: {type: integer, min: 0, max: 1000}` and go on to infer a
//! property the ontology had never heard of, and nothing anywhere would
//! say so.
//!
//! # Why that matters more than it sounds
//!
//! A rule referencing a property that does not exist does not fail. It
//! evaluates to `Unknown`, so the rule quietly never fires, and the run
//! produces a perfectly ordinary-looking answer that simply left out a
//! step. A typo in a subject is therefore invisible at every level the
//! engine otherwise checks: the YAML parses, the rule loads, the
//! evaluation is well defined, the proof records that the rule was tried.
//!
//! That is the failure mode this whole project exists to prevent — a
//! decision quietly made on absent data — arriving through the one door
//! nothing was watching.
//!
//! # What this type does and does not do
//!
//! It describes declared types and domains. **This type** enforces nothing:
//! nothing here narrows a value or rejects an observation, and `dre validate`
//! reads it as a separate pass with no path into a decision.
//!
//! ⚠ BUT ENFORCEMENT IS NO LONGER VALIDATION-ONLY, and this paragraph said it
//! was for a day after it stopped being true. It used to read *"Enforcement
//! lives in `dre validate`"* and go on to argue that checking during a run
//! would be a reasoning change deserving its own pass. **That pass happened —
//! twice.** Engine 0.7.0 began rejecting a reading whose kind, domain or
//! vocabulary the ontology does not admit, and 0.22.0 added the arm it had
//! missed: a reading naming a predicate the entity's type does not declare.
//! Both live in `dre-lab`'s runner (`defect_in`), not in this crate, so "this
//! type enforces nothing" survives — the claim that *nothing does* did not.
//!
//! The old reasoning was right and only its conclusion expired: each bump WAS
//! a reasoning change, and each got its own version and its own re-recorded
//! baselines. The corollary still holds for anything new — bounds checking
//! that changes what the engine concludes belongs behind a version bump, never
//! smuggled in as a validation feature.
//!
//! ⛔ This was the SECOND copy of the stale claim; `docs/decisions.md` carried
//! it too and was struck through on 2026-08-16. A fact asserted in two places
//! goes stale in two places and gets corrected in one, so the one that is
//! easiest to reach is the one that misleads longest.
//!
//! Not stale, and worth not "fixing": `runner.rs`'s note that validation "walks
//! entities and never touches `given.evidence`" is still accurate —
//! `validate::experiment` genuinely does not walk evidence. That gap is open by
//! measurement, not by oversight: zero rows in the corpus exercise it.

use dre_core::collections::DetMap;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};

use crate::Value;

/// What kind of value a property holds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    /// Whole number.
    Integer,
    /// Fixed-point number.
    Fixed,
    /// Boolean.
    Boolean,
    /// Symbol drawn from the domain's vocabulary.
    Symbol,
    /// Reference to another entity.
    Reference,
}

impl PropertyKind {
    /// Whether a value is of this kind.
    ///
    /// `Integer` accepts a fixed-point value and vice versa, because the
    /// engine converts freely between them — a feature dividing two
    /// integers produces `Fixed`, and requiring the pack to re-declare the
    /// property's kind as a consequence of *how* it happens to be computed
    /// would make the ontology track the implementation.
    #[must_use]
    pub const fn accepts(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (
                Self::Integer | Self::Fixed,
                Value::Integer(_) | Value::Fixed(_)
            ) | (Self::Boolean, Value::Boolean(_))
                | (Self::Symbol, Value::Symbol(_))
                | (Self::Reference, Value::Reference(_))
        )
    }

    /// How this kind is written in a pack.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Fixed => "fixed",
            Self::Boolean => "boolean",
            Self::Symbol => "symbol",
            Self::Reference => "reference",
        }
    }
}

/// A declared property: its kind, and the domain it is defined over.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyType {
    /// What kind of value it holds.
    #[serde(rename = "type")]
    pub kind: PropertyKind,

    /// Inclusive lower bound, for numeric properties.
    #[serde(default)]
    pub min: Option<Fixed>,

    /// Inclusive upper bound, for numeric properties.
    #[serde(default)]
    pub max: Option<Fixed>,

    /// The symbols this property may hold.
    ///
    /// Absent means open — any symbol is admissible — exactly as absent
    /// `min`/`max` means unbounded. Declaring the list closes the
    /// vocabulary, which is what lets validation catch `condition:
    /// degarded`: a typo in a *value* is as invisible at run time as a typo
    /// in a subject, since a symbol nothing compares equal to simply makes
    /// every rule reading it fail to fire.
    ///
    /// `Option` rather than a defaulted `Vec` so that a declared-but-empty
    /// list stays distinguishable from no declaration at all. An empty
    /// vocabulary admits nothing, which is almost certainly a mistake, and
    /// validation says so rather than silently treating it as open.
    #[serde(default)]
    pub values: Option<Vec<Id>>,

    /// Which value wins when two rules infer different ones, most dominant
    /// first.
    ///
    /// Absent — the default and the behaviour every existing pack gets — two
    /// rules concluding different values give `CONFLICTING` and the value is
    /// discarded. That is correct for a *belief*: two sources disagreeing
    /// about a temperature does not make one of them right, and priority
    /// deciding it would let rule ordering silently manufacture facts.
    ///
    /// It is wrong for a *policy*. An access decision where one rule says
    /// `deny` and another says `manual_review` is not undecided; `deny`
    /// dominates, and it dominates whichever rule happened to fire first or
    /// carry the higher priority. The fourth independent author hit exactly
    /// this modelling OPC UA access control, and left the clash in their
    /// experiment rather than fudge it — the second independent report of
    /// the class after `device-cascade`.
    ///
    /// So dominance is **declared on the value domain**, not inferred from
    /// rule priority. `[deny, manual_review, allow]` says what the words
    /// mean relative to each other, which is a fact about the domain that
    /// belongs in the ontology and is covered by `ModelHash`. Rule priority
    /// still orders *firing* and still does not decide *merging*: a pack
    /// that wants precedence has to say so, in the one place that describes
    /// what the values are.
    ///
    /// Applies to inference only. Observations disagreeing is an evidence
    /// question with provenance to reason over, and is left alone.
    #[serde(default)]
    pub precedence: Option<Vec<Id>>,
}

impl PropertyType {
    /// Read a value as a number, if it is one.
    fn numeric(value: &Value) -> Option<Fixed> {
        match value {
            Value::Integer(inner) => Fixed::from_int(*inner).ok(),
            Value::Fixed(inner) => Some(*inner),
            _ => None,
        }
    }

    /// Whether a value lies within the declared domain.
    ///
    /// A property with no bounds is unbounded, and a non-numeric value has
    /// no bounds to violate — both return `true` rather than being treated
    /// as failures, because "no constraint declared" is not a constraint.
    #[must_use]
    pub fn in_domain(&self, value: &Value) -> bool {
        let Some(number) = Self::numeric(value) else {
            return true;
        };
        if self.min.is_some_and(|min| number < min) {
            return false;
        }
        !self.max.is_some_and(|max| number > max)
    }

    /// Describe the declared domain, for a diagnostic.
    #[must_use]
    pub fn domain(&self) -> String {
        match (self.min, self.max) {
            (Some(min), Some(max)) => format!("[{min}, {max}]"),
            (Some(min), None) => format!("[{min}, ..)"),
            (None, Some(max)) => format!("(.., {max}]"),
            (None, None) => "unbounded".to_owned(),
        }
    }

    /// Whether a symbol is in the declared vocabulary.
    ///
    /// A property with no declared vocabulary admits any symbol, and a
    /// non-symbol value has no vocabulary to violate — both `true`, for the
    /// same reason [`in_domain`](Self::in_domain) is permissive: no
    /// constraint declared is not a constraint.
    #[must_use]
    pub fn admits(&self, value: &Value) -> bool {
        let Value::Symbol(symbol) = value else {
            return true;
        };
        self.values
            .as_ref()
            .is_none_or(|allowed| allowed.contains(symbol))
    }

    /// Describe the declared vocabulary, for a diagnostic.
    #[must_use]
    pub fn vocabulary(&self) -> String {
        match self.values.as_ref() {
            None => "open".to_owned(),
            Some(allowed) if allowed.is_empty() => "empty".to_owned(),
            Some(allowed) => allowed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// A declared entity type.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityType {
    /// Properties instances of this type may carry.
    #[serde(default)]
    pub properties: DetMap<Id, PropertyType>,
}

/// A declared relationship type.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipType {
    /// Entity type the relationship runs from.
    pub from: Id,
    /// Entity type it runs to.
    pub to: Id,

    /// How many `to` entities one `from` may reach.
    #[serde(default)]
    pub targets_per_source: Cardinality,

    /// How many `from` entities may reach one `to`.
    #[serde(default)]
    pub sources_per_target: Cardinality,
}

/// How many entities one side of a relationship may reach.
///
/// Two explicit fields rather than a single `one_to_many` spelling, because
/// which side the "one" refers to in that spelling is precisely the sort of
/// thing this repository has been burned by. Both default to `Many`, the
/// weakest claim, so the packs that already declare a `relationships:` block
/// parse byte-identically and gain no guarantee they did not ask for.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// At most one.
    One,
    /// Any number.
    #[default]
    Many,
}

/// The `ontology.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntologySection {
    /// Declared entity types.
    #[serde(default)]
    pub entities: DetMap<Id, EntityType>,

    /// Declared relationship types.
    #[serde(default)]
    pub relationships: DetMap<Id, RelationshipType>,
}

impl OntologySection {
    /// Declared value precedence, keyed `type.property`, most dominant
    /// first.
    ///
    /// Empty when no property declares one, which is every pack written
    /// before engine 0.14.0 — so the table being empty is the old behaviour
    /// rather than a special case anybody has to remember.
    #[must_use]
    pub fn precedence(&self) -> dre_core::collections::DetMap<Id, Vec<Id>> {
        let mut table = dre_core::collections::DetMap::new();
        for (type_id, entity) in self.entities.iter() {
            for (property, declared) in entity.properties.iter() {
                let Some(order) = declared.precedence.as_ref() else {
                    continue;
                };
                if let Ok(key) = Id::new(format!("{type_id}.{property}")) {
                    table.insert(key, order.clone());
                }
            }
        }
        table
    }

    /// Look up a property on a named entity type.
    #[must_use]
    pub fn property_of(&self, entity_type: &Id, property: &Id) -> Option<&PropertyType> {
        self.entities.get(entity_type)?.properties.get(property)
    }

    /// Whether any declared entity type carries this property name.
    ///
    /// The weaker check, and the only one available without an experiment:
    /// a pack's subjects name entity **instances**, and which type an
    /// instance has is something only the experiment says. The `logic` pack
    /// is the case that proves this is not pedantry — it declares one type
    /// `proposition` and its rules talk about `a.holds`, `b.holds` and
    /// `c.holds`.
    #[must_use]
    pub fn declares_property(&self, property: &Id) -> bool {
        self.entities
            .values()
            .any(|entity| entity.properties.contains_key(property))
    }

    /// Every entity type declaring a property of this name.
    #[must_use]
    pub fn types_declaring(&self, property: &Id) -> Vec<&Id> {
        self.entities
            .iter()
            .filter(|(_, entity)| entity.properties.contains_key(property))
            .map(|(id, _)| id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn bounded(min: i64, max: i64) -> PropertyType {
        PropertyType {
            kind: PropertyKind::Integer,
            min: Some(Fixed::from_int(min).expect("in range")),
            max: Some(Fixed::from_int(max).expect("in range")),
            values: None,
            precedence: None,
        }
    }

    fn symbol(values: Option<&[&str]>) -> PropertyType {
        PropertyType {
            kind: PropertyKind::Symbol,
            min: None,
            max: None,
            values: values.map(|names| names.iter().map(|name| id(name)).collect()),
            precedence: None,
        }
    }

    #[test]
    fn an_open_vocabulary_admits_any_symbol() {
        // Absent means open, exactly as absent min and max mean unbounded.
        let property = symbol(None);
        assert!(property.admits(&Value::Symbol(id("anything-at-all"))));
        assert_eq!(property.vocabulary(), "open");
    }

    #[test]
    fn a_declared_vocabulary_is_closed() {
        // The typo this exists to catch. `degarded` compares equal to
        // nothing, so at run time every rule reading it simply fails to
        // fire and the run looks ordinary.
        let property = symbol(Some(&["degraded", "healthy"]));
        assert!(property.admits(&Value::Symbol(id("degraded"))));
        assert!(!property.admits(&Value::Symbol(id("degarded"))));
        assert_eq!(property.vocabulary(), "degraded, healthy");
    }

    #[test]
    fn a_declared_but_empty_vocabulary_stays_distinguishable() {
        // `Option` rather than a defaulted `Vec` so this case exists at
        // all. It admits nothing, which is almost certainly a mistake, and
        // silently treating it as open would hide the mistake.
        let property = symbol(Some(&[]));
        assert!(!property.admits(&Value::Symbol(id("anything"))));
        assert_eq!(property.vocabulary(), "empty");
    }

    #[test]
    fn a_non_symbol_has_no_vocabulary_to_violate() {
        // A number on a symbol property is a *kind* error, reported
        // separately. One mistake, one voice.
        assert!(symbol(Some(&["degraded"])).admits(&Value::Integer(7)));
    }

    #[test]
    fn bounds_are_inclusive_at_both_ends() {
        let property = bounded(0, 1_000);
        assert!(property.in_domain(&Value::Integer(0)));
        assert!(property.in_domain(&Value::Integer(1_000)));
        assert!(!property.in_domain(&Value::Integer(-1)));
        assert!(!property.in_domain(&Value::Integer(1_001)));
    }

    #[test]
    fn an_unbounded_property_admits_anything_numeric() {
        let property = PropertyType {
            kind: PropertyKind::Integer,
            min: None,
            max: None,
            values: None,
            precedence: None,
        };
        assert!(property.in_domain(&Value::Integer(i64::MIN)));
        assert_eq!(property.domain(), "unbounded");
    }

    #[test]
    fn a_non_numeric_value_has_no_bounds_to_violate() {
        // "No constraint declared" is not a constraint. A symbol on a
        // property carrying min and max is a *kind* error, reported
        // separately -- reporting it as a range violation too would give
        // one mistake two voices.
        assert!(bounded(0, 10).in_domain(&Value::Symbol(id("grade-a"))));
    }

    #[test]
    fn integer_and_fixed_are_interchangeable_kinds() {
        // A feature dividing two integers produces Fixed. Requiring the
        // pack to re-declare the property's kind because of how it happens
        // to be computed would make the ontology track the implementation.
        assert!(PropertyKind::Integer.accepts(&Value::Fixed(Fixed::ONE)));
        assert!(PropertyKind::Fixed.accepts(&Value::Integer(1)));
        assert!(!PropertyKind::Integer.accepts(&Value::Boolean(true)));
        assert!(!PropertyKind::Symbol.accepts(&Value::Reference(id("pump-1"))));
    }

    /// Two types, one boolean property each side, one bounded number.
    ///
    /// Built in code rather than parsed: `dre-state` cannot depend on
    /// `dre-model`, which owns the strict YAML loader, and the lookup logic
    /// is what these tests are about.
    fn fixture() -> OntologySection {
        let mut proposition = EntityType::default();
        proposition.properties.insert(
            id("holds"),
            PropertyType {
                kind: PropertyKind::Boolean,
                min: None,
                max: None,
                values: None,
                precedence: None,
            },
        );

        let mut machine = EntityType::default();
        machine
            .properties
            .insert(id("temperature"), bounded(0, 1_000));

        let mut ontology = OntologySection::default();
        ontology.entities.insert(id("proposition"), proposition);
        ontology.entities.insert(id("machine"), machine);
        ontology
    }

    #[test]
    fn a_property_is_found_by_name_across_types() {
        // The weak check, and the one a pack alone can support. `logic`
        // declares one type and names three instances of it, so a subject's
        // first segment is not a type name.
        let ontology = fixture();

        assert!(ontology.declares_property(&id("holds")));
        assert!(ontology.declares_property(&id("temperature")));
        assert!(
            !ontology.declares_property(&id("temprature")),
            "a typo must not resolve"
        );

        assert_eq!(
            ontology.types_declaring(&id("holds")),
            vec![&id("proposition")]
        );
    }

    #[test]
    fn a_property_on_the_wrong_type_does_not_resolve() {
        // The strong check, available once an experiment binds instances to
        // types. `machine.holds` is a real property name on a real type and
        // still nonsense.
        let ontology = fixture();

        assert!(ontology
            .property_of(&id("machine"), &id("temperature"))
            .is_some());
        assert!(ontology.property_of(&id("machine"), &id("holds")).is_none());
        assert!(ontology
            .property_of(&id("turbine"), &id("temperature"))
            .is_none());
    }
}
