//! Phase 2 — Canonical state.
//!
//! The universal representation of a reasoning problem. Every model pack
//! compiles down to this, and every reasoner reads it, which is what keeps
//! domain knowledge out of the engine: the symbolic reasoner cannot know
//! whether a property means a temperature or a grid coordinate.
//!
//! The distinctive commitment here is that *not knowing* is a first-class
//! state rather than an absence. A property whose value is unknown, or
//! whose sources conflict, is represented explicitly (see
//! [`EpistemicStatus`]). Systems that model missing data as null tend to
//! collapse "we have not measured this", "this cannot be measured", and
//! "our two sensors disagree" into one indistinguishable case — and those
//! three situations should drive quite different reasoning.
//!
//! Status: skeleton. Types are declared; the state builder is not yet
//! implemented. See `docs/roadmap.md`.

pub mod builder;
pub mod error;
pub mod ontology;
mod value_serde;

pub use builder::StateBuilder;
pub use error::StateError;
pub use ontology::{EntityType, OntologySection, PropertyKind, PropertyType};

use dre_core::canonical::{Canonical, CanonicalWriter};
use dre_core::collections::DetMap;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};

/// What the engine knows about a property.
///
/// Ordering is fixed and meaningful: it is the precedence used when
/// merging observations, so it must not be reordered casually.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EpistemicStatus {
    /// Directly observed, or inferred with sufficient supporting evidence.
    Known,
    /// Not observed, but observable in principle.
    Unknown,
    /// Derived by a rule rather than observed.
    Inferred,
    /// Sources disagree and the conflict is unresolved.
    Conflicting,
    /// Observed, but the observation is older than the model's freshness
    /// window.
    Stale,
    /// Cannot be observed in this domain — distinct from merely unobserved,
    /// because no amount of evidence gathering will resolve it.
    Unobservable,
    /// Observed but outside the declared domain of the property.
    Invalid,
    /// Meaningless for this entity, such as engine temperature on a
    /// component that has no engine.
    NotApplicable,
}

/// A typed property value.
///
/// Deliberately excludes floating point; see `dre_core::fixed` for why.
///
/// Serde impls are hand-written in `value_serde` so packs read naturally.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Value {
    /// Whole number.
    Integer(i64),
    /// Fixed-point number.
    Fixed(Fixed),
    /// Boolean.
    Boolean(bool),
    /// Text, drawn from a declared enumeration in the model pack.
    Symbol(Id),
    /// Reference to another entity.
    Reference(Id),
}

impl Canonical for Value {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("value");
        match self {
            Self::Integer(value) => {
                out.write_str("integer").write_i64(*value);
            }
            Self::Fixed(value) => {
                out.write_str("fixed").write(value);
            }
            Self::Boolean(value) => {
                out.write_str("boolean").write_bool(*value);
            }
            Self::Symbol(value) => {
                out.write_str("symbol").write(value);
            }
            Self::Reference(value) => {
                out.write_str("reference").write(value);
            }
        }
    }
}

/// A property of an entity, together with what is known about it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Property {
    /// Property name, unique within its entity.
    pub name: Id,
    /// The value, absent when the status carries no value.
    pub value: Option<Value>,
    /// What the engine knows about this property.
    pub status: EpistemicStatus,
}

impl Canonical for Property {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("property").write(&self.name);
        out.write_option(self.value.as_ref());
        out.write_str(status_tag(self.status));
    }
}

/// A typed entity.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier.
    pub id: Id,
    /// Entity type, declared by the model pack's ontology.
    pub type_id: Id,
    /// Properties, ordered by name.
    pub properties: DetMap<Id, Property>,
}

impl Canonical for Entity {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("entity")
            .write(&self.id)
            .write(&self.type_id)
            .write(&self.properties);
    }
}

/// A typed directed relationship between two entities.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Relationship {
    /// Unique identifier.
    pub id: Id,
    /// Relationship type, declared by the model pack's ontology.
    pub type_id: Id,
    /// Source entity.
    pub from: Id,
    /// Target entity.
    pub to: Id,
}

impl Canonical for Relationship {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("relationship")
            .write(&self.id)
            .write(&self.type_id)
            .write(&self.from)
            .write(&self.to);
    }
}

/// The complete state of a reasoning problem.
///
/// Ordered collections throughout, so the canonical encoding — and
/// therefore the `InputHash` — does not depend on the order in which the
/// model pack happened to declare things.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct CanonicalState {
    /// Entities, ordered by identifier.
    pub entities: DetMap<Id, Entity>,
    /// Relationships, ordered by identifier.
    pub relationships: DetMap<Id, Relationship>,
}

impl Canonical for CanonicalState {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("canonical-state")
            .write(&self.entities)
            .write(&self.relationships);
    }
}

/// Stable string tag for an epistemic status, used in canonical encoding.
///
/// Written out explicitly rather than derived from the enum name so that
/// renaming a variant cannot silently change a stored `InputHash`.
const fn status_tag(status: EpistemicStatus) -> &'static str {
    match status {
        EpistemicStatus::Known => "KNOWN",
        EpistemicStatus::Unknown => "UNKNOWN",
        EpistemicStatus::Inferred => "INFERRED",
        EpistemicStatus::Conflicting => "CONFLICTING",
        EpistemicStatus::Stale => "STALE",
        EpistemicStatus::Unobservable => "UNOBSERVABLE",
        EpistemicStatus::Invalid => "INVALID",
        EpistemicStatus::NotApplicable => "NOT_APPLICABLE",
    }
}

impl CanonicalState {
    /// Resolve an entity name to the entity it can only mean.
    ///
    /// **The one place this rule lives.** An exact instance name wins; a name
    /// no instance answers to resolves to the sole instance of that type, if
    /// there is exactly one; a type with several instances resolves to
    /// nothing, because which one a bare name means is a question with no
    /// declared answer.
    ///
    /// It is here, on the state, rather than in each reasoner because the
    /// alternative was measured. Type resolution was added section by
    /// section, and by 0.15.0 the workspace held two binding implementations,
    /// three byte-identical copies of this fallback, and several bare lookups
    /// with no fallback at all. Three independently authored packs found the
    /// gaps, in three different sections, and a fourth defect survived 0.15.0
    /// inside a function that had already been fixed on its success path.
    ///
    /// A rule that lives in one place cannot be forgotten by a section added
    /// tomorrow. That is the whole argument.
    #[must_use]
    pub fn resolve_entity(&self, name: &str) -> Option<&Entity> {
        if let Some((_, entity)) = self.entities.iter().find(|(id, _)| id.as_str() == name) {
            return Some(entity);
        }
        let mut found: Option<&Entity> = None;
        for entity in self.entities.values() {
            if entity.type_id.as_str() != name {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(entity);
        }
        found
    }

    /// Resolve an entity name to its identifier, unchanged when it resolves
    /// to nothing.
    ///
    /// The write-side companion to [`Self::resolve_entity`]. Leaving an
    /// unresolvable name alone rather than substituting one is deliberate:
    /// the write then fails loudly against the name the pack actually wrote,
    /// which is the error a reader can act on.
    #[must_use]
    pub fn resolve_entity_id(&self, name: Id) -> Id {
        if self.entities.contains_key(&name) {
            return name;
        }
        let mut found: Option<Id> = None;
        for (id, entity) in self.entities.iter() {
            if entity.type_id != name {
                continue;
            }
            if found.is_some() {
                return name;
            }
            found = Some(id.clone());
        }
        found.unwrap_or(name)
    }

    /// A property by dotted subject, resolving the entity half.
    #[must_use]
    pub fn property(&self, subject: &Id) -> Option<&Property> {
        let (entity_name, property_name) = subject.as_str().rsplit_once('.')?;
        self.resolve_entity(entity_name)?
            .properties
            .iter()
            .find(|(id, _)| id.as_str() == property_name)
            .map(|(_, property)| property)
    }
}

/// A value as it reads in a proof or a report.
///
/// Here rather than in the lab crate because the proof is written by engine
/// code, and a node whose text depended on a formatting helper the harness
/// owned would put presentation inside `ProofHash`.
#[must_use]
pub fn render_value(value: &Value) -> String {
    match value {
        Value::Boolean(inner) => inner.to_string(),
        Value::Integer(inner) => inner.to_string(),
        Value::Fixed(inner) => inner.to_string(),
        Value::Symbol(inner) | Value::Reference(inner) => inner.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn property(name: &str, value: Option<Value>, status: EpistemicStatus) -> Property {
        Property {
            name: Id::new(name).expect("valid id"),
            value,
            status,
        }
    }

    #[test]
    fn unknown_and_unobservable_are_distinguishable() {
        let unknown = property("internal_state", None, EpistemicStatus::Unknown);
        let unobservable = property("internal_state", None, EpistemicStatus::Unobservable);
        // Both lack a value, but they should never hash alike: one invites
        // further evidence gathering and the other forecloses it.
        assert_ne!(unknown.canonical_hash(), unobservable.canonical_hash());
    }

    #[test]
    fn state_hash_is_declaration_order_independent() {
        let build = |names: [&str; 2]| {
            let mut state = CanonicalState::default();
            for name in names {
                let id = Id::new(name).expect("valid id");
                state.entities.insert(
                    id.clone(),
                    Entity {
                        id,
                        type_id: Id::new("generic-object").expect("valid id"),
                        properties: DetMap::new(),
                    },
                );
            }
            state
        };

        assert_eq!(
            build(["object-a", "object-b"]).canonical_hash(),
            build(["object-b", "object-a"]).canonical_hash()
        );
    }
}
