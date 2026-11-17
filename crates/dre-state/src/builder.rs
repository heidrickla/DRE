//! Deterministic state builder.
//!
//! Assembles [`CanonicalState`] from declarations that may arrive in any
//! order and may say the same thing more than once.
//!
//! # The merge rule
//!
//! The whole crate turns on one question: when two facts about the same
//! property arrive, what happens? The answer has to be a *total,
//! commutative* function, because the order facts arrive in is an accident
//! of how a model pack was written and must not reach the result.
//!
//! Two channels feed a property. An **observation** is something a source
//! claims to have measured; an **inference** is something a rule derived.
//! Observation outranks inference — a rule that concluded the bearing is
//! fine does not override a sensor that measured otherwise.
//!
//! | prior | incoming | result |
//! |---|---|---|
//! | unknown | observe `v` | `KNOWN v` |
//! | `KNOWN v` | observe `v` | `KNOWN v` |
//! | `KNOWN a` | observe `b` | `CONFLICTING` |
//! | `INFERRED _` | observe `v` | `KNOWN v` |
//! | unknown | infer `v` | `INFERRED v` |
//! | `INFERRED v` | infer `v` | `INFERRED v` |
//! | `INFERRED a` | infer `b` | `CONFLICTING` |
//! | `KNOWN v` | infer anything | `KNOWN v` |
//! | `CONFLICTING` | anything | `CONFLICTING` |
//!
//! Check commutativity on the interesting pair: observe `a` then infer `b`
//! gives `KNOWN a`, because inference cannot displace an observation.
//! Infer `b` then observe `a` gives `INFERRED b` then `KNOWN a` — the same
//! answer. Conflict is likewise symmetric, and absorbing once reached.
//!
//! A conflicting property carries **no value**. Retaining one would mean
//! picking a winner, and the honest report is that the engine does not
//! know. Which sources disagreed is evidence's business, not state's.

use dre_core::collections::DetMap;
use dre_core::config::Limits;
use dre_core::id::Id;

use crate::error::{Result, StateError};
use crate::ontology::{Cardinality, RelationshipType};
use crate::{CanonicalState, Entity, EpistemicStatus, Property, Relationship, Value};

/// Accumulates declarations into canonical state.
#[derive(Clone, Debug)]
pub struct StateBuilder {
    entities: DetMap<Id, EntityDraft>,
    relationships: DetMap<Id, Relationship>,
    limits: Limits,
    features: u64,
    precedence: DetMap<Id, Vec<Id>>,
    overruled: Vec<(Id, Value, Value)>,
    relationship_types: DetMap<Id, RelationshipType>,
}

#[derive(Clone, Debug)]
struct EntityDraft {
    type_id: Id,
    properties: DetMap<Id, Property>,
}

impl StateBuilder {
    /// Start a builder under the given ceilings.
    #[must_use]
    pub fn new(limits: Limits) -> Self {
        Self {
            entities: DetMap::new(),
            relationships: DetMap::new(),
            limits,
            features: 0,
            precedence: DetMap::new(),
            overruled: Vec::new(),
            relationship_types: DetMap::new(),
        }
    }

    /// Declare which value dominates when two inferences disagree.
    ///
    /// Keyed `type.property`, ordered most dominant first, and empty by
    /// default so a pack that declares nothing keeps the conflict behaviour
    /// it always had. See `PropertyType::precedence`.
    #[must_use]
    pub fn with_precedence(mut self, precedence: DetMap<Id, Vec<Id>>) -> Self {
        self.precedence = precedence;
        self
    }

    /// Conflicts precedence settled, as `(subject, winner, loser)`.
    ///
    /// Drained rather than returned from `infer`, so recording a dominance
    /// decision in the proof costs no change to the signature every caller
    /// uses. A resolution nothing records would be a value appearing from
    /// nowhere, which is the one thing this engine must not do.
    pub fn take_overruled(&mut self) -> Vec<(Id, Value, Value)> {
        std::mem::take(&mut self.overruled)
    }

    /// Start a builder under the MVP default ceilings.
    #[must_use]
    pub fn with_default_limits() -> Self {
        Self::new(Limits::MVP)
    }

    /// Reopen already-built state for further merging.
    ///
    /// Forward chaining needs this: each round adds inferences to the
    /// state produced by the previous one, and every addition must go
    /// through the same merge rule as the original declarations. Rebuilding
    /// through the builder is what guarantees that — an inference written
    /// directly into the state map would bypass the precedence rules and
    /// could overwrite an observation.
    #[must_use]
    pub fn from_state(state: CanonicalState, limits: Limits) -> Self {
        let mut features: u64 = 0;
        let mut entities: DetMap<Id, EntityDraft> = DetMap::new();

        for (id, entity) in state.entities {
            features = features.saturating_add(entity.properties.len() as u64);
            entities.insert(
                id,
                EntityDraft {
                    type_id: entity.type_id,
                    properties: entity.properties,
                },
            );
        }

        Self {
            entities,
            relationships: state.relationships,
            limits,
            features,
            precedence: DetMap::new(),
            overruled: Vec::new(),
            relationship_types: DetMap::new(),
        }
    }

    /// Declare an entity.
    ///
    /// Declaring the same identifier twice is an error rather than an
    /// overwrite: a model pack that names one entity twice is ambiguous
    /// about which type it has, and silently keeping the last would make
    /// the result depend on file order.
    pub fn entity(&mut self, id: Id, type_id: Id) -> Result<&mut Self> {
        if self.entities.contains_key(&id) {
            return Err(StateError::DuplicateEntity { entity: id });
        }
        if self.entities.len() as u64 >= self.limits.max_entities {
            return Err(StateError::LimitExceeded {
                limit: "max_entities",
                cap: self.limits.max_entities,
            });
        }
        self.entities.insert(
            id,
            EntityDraft {
                type_id,
                properties: DetMap::new(),
            },
        );
        Ok(self)
    }

    /// Record an observed value.
    pub fn observe(&mut self, entity: &Id, property: Id, value: Value) -> Result<&mut Self> {
        self.merge(entity, property, Some(value), Channel::Observation)
    }

    /// Replace a property outright, discarding whatever was there.
    ///
    /// This is what a **causal effect** needs, and it is emphatically not
    /// [`observe`](Self::observe).
    ///
    /// The merge rule accumulates evidence about a world assumed to hold
    /// still: two observations of one temperature that disagree mean a
    /// sensor is wrong, so the result is `CONFLICTING`. An action changes
    /// the world instead. Moving an agent from `x = 0` to `x = 1` is not a
    /// second opinion about `x`; the old value is superseded, not
    /// contradicted.
    ///
    /// Routing effects through `observe` makes every state transition
    /// conflict with its own predecessor, which then makes every
    /// subsequent comparison undecidable and stalls a planner one step in.
    pub fn set(
        &mut self,
        entity: &Id,
        property: Id,
        value: Option<Value>,
        status: EpistemicStatus,
    ) -> Result<&mut Self> {
        let added = {
            let draft = Self::draft_mut(&mut self.entities, entity, &property)?;
            let replaced = draft.properties.insert(
                property.clone(),
                Property {
                    name: property,
                    value,
                    status,
                },
            );
            replaced.is_none()
        };
        if added {
            self.features = self.features.saturating_add(1);
        }
        self.check_features()
    }

    /// Record a derived value.
    pub fn infer(&mut self, entity: &Id, property: Id, value: Value) -> Result<&mut Self> {
        self.merge(entity, property, Some(value), Channel::Inference)
    }

    /// Declare a property that exists but holds no value.
    ///
    /// Used for `UNKNOWN`, `UNOBSERVABLE` and `NOT_APPLICABLE`, which are
    /// distinct claims and must be declarable explicitly rather than
    /// inferred from absence.
    pub fn declare(
        &mut self,
        entity: &Id,
        property: Id,
        status: EpistemicStatus,
    ) -> Result<&mut Self> {
        let added = {
            let draft = Self::draft_mut(&mut self.entities, entity, &property)?;
            if draft.properties.contains_key(&property) {
                false
            } else {
                draft.properties.insert(
                    property.clone(),
                    Property {
                        name: property,
                        value: None,
                        status,
                    },
                );
                true
            }
        };
        if added {
            self.features = self.features.saturating_add(1);
        }
        self.check_features()
    }

    /// Declare the relationship types edges are checked against.
    ///
    /// Empty by default, which is the behaviour every pack had before edges
    /// were declarable: endpoints are checked for existence and nothing is
    /// checked about their types.
    #[must_use]
    pub fn with_relationship_types(mut self, types: DetMap<Id, RelationshipType>) -> Self {
        self.relationship_types = types;
        self
    }

    /// Declare a relationship between two entities.
    ///
    /// Written in 2026 and called by nothing until edges became declarable.
    /// `RelationshipType.from`/`to` had been declared, hashed into every
    /// `ModelHash` and read by nothing for just as long — two packs declared
    /// a `relationships:` block and the brief admitted out loud that it was
    /// "declared but not yet checked". Checking it here rather than in
    /// `dre validate` is deliberate: validate is a separate pass with no path
    /// into a decision, so a check that only lives there is not a refusal.
    pub fn relate(&mut self, id: Id, type_id: Id, from: Id, to: Id) -> Result<&mut Self> {
        if self.relationships.contains_key(&id) {
            return Err(StateError::DuplicateRelationship { relationship: id });
        }
        for endpoint in [&from, &to] {
            if !self.entities.contains_key(endpoint) {
                return Err(StateError::UnknownRelationshipEndpoint {
                    relationship: id.clone(),
                    entity: endpoint.clone(),
                });
            }
        }

        if let Some(declared) = self.relationship_types.get(&type_id) {
            // Endpoint types, checked against what the ontology declared.
            for (endpoint, expected, side) in
                [(&from, &declared.from, "from"), (&to, &declared.to, "to")]
            {
                let actual = self
                    .entities
                    .get(endpoint)
                    .map(|draft| draft.type_id.clone());
                if actual.as_ref() != Some(expected) {
                    return Err(StateError::RelationshipEndpointType {
                        relationship: id.clone(),
                        entity: endpoint.clone(),
                        side,
                        declared: expected.clone(),
                        actual: actual.unwrap_or_else(|| expected.clone()),
                    });
                }
            }

            // Declared cardinality, enforced rather than described.
            let existing = |pick: fn(&Relationship) -> &Id, endpoint: &Id| {
                self.relationships
                    .values()
                    .any(|edge| edge.type_id == type_id && pick(edge) == endpoint)
            };
            if declared.targets_per_source == Cardinality::One && existing(|edge| &edge.from, &from)
            {
                return Err(StateError::ArityViolated {
                    relationship_type: type_id,
                    entity: from,
                    side: "targets_per_source: one",
                });
            }
            if declared.sources_per_target == Cardinality::One && existing(|edge| &edge.to, &to) {
                return Err(StateError::ArityViolated {
                    relationship_type: type_id,
                    entity: to,
                    side: "sources_per_target: one",
                });
            }
        }
        self.relationships.insert(
            id.clone(),
            Relationship {
                id,
                type_id,
                from,
                to,
            },
        );
        Ok(self)
    }

    /// Finish and produce canonical state.
    #[must_use]
    pub fn build(self) -> CanonicalState {
        let entities = self
            .entities
            .into_iter()
            .map(|(id, draft)| {
                let entity = Entity {
                    id: id.clone(),
                    type_id: draft.type_id,
                    properties: draft.properties,
                };
                (id, entity)
            })
            .collect();

        CanonicalState {
            entities,
            relationships: self.relationships,
        }
    }

    /// Borrow an entity draft.
    ///
    /// Takes the map rather than `&mut self` so callers can still touch
    /// other fields of the builder while holding the draft.
    fn draft_mut<'a>(
        entities: &'a mut DetMap<Id, EntityDraft>,
        entity: &Id,
        property: &Id,
    ) -> Result<&'a mut EntityDraft> {
        entities
            .get_mut(entity)
            .ok_or_else(|| StateError::UnknownEntity {
                entity: entity.clone(),
                property: property.clone(),
            })
    }

    fn check_features(&mut self) -> Result<&mut Self> {
        if self.features > self.limits.max_features {
            return Err(StateError::LimitExceeded {
                limit: "max_features",
                cap: self.limits.max_features,
            });
        }
        Ok(self)
    }

    fn merge(
        &mut self,
        entity: &Id,
        property: Id,
        value: Option<Value>,
        channel: Channel,
    ) -> Result<&mut Self> {
        let mut overruled: Option<(Id, Value, Value)> = None;
        let added = {
            let precedence = &self.precedence;
            let draft = Self::draft_mut(&mut self.entities, entity, &property)?;
            let order = Id::new(format!("{}.{}", draft.type_id, property))
                .ok()
                .and_then(|key| precedence.get(&key));
            match draft.properties.get_mut(&property) {
                Some(existing) => {
                    if let Some(loser) = merge_into(existing, value, channel, order) {
                        if let (Ok(subject), Some(winner)) = (
                            Id::new(format!("{entity}.{property}")),
                            existing.value.clone(),
                        ) {
                            overruled = Some((subject, winner, loser));
                        }
                    }
                    false
                }
                None => {
                    draft.properties.insert(
                        property.clone(),
                        Property {
                            name: property,
                            value,
                            status: channel.initial_status(),
                        },
                    );
                    true
                }
            }
        };
        if let Some(event) = overruled {
            self.overruled.push(event);
        }
        if added {
            self.features = self.features.saturating_add(1);
        }
        self.check_features()
    }
}

/// Which channel a fact arrived on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Observation,
    Inference,
}

impl Channel {
    const fn initial_status(self) -> EpistemicStatus {
        match self {
            Self::Observation => EpistemicStatus::Known,
            Self::Inference => EpistemicStatus::Inferred,
        }
    }
}

/// Rank a value within a declared precedence order, if it is in one.
fn rank(order: Option<&Vec<Id>>, value: Option<&Value>) -> Option<usize> {
    let order = order?;
    let Some(Value::Symbol(symbol)) = value else {
        return None;
    };
    order.iter().position(|candidate| candidate == symbol)
}

/// Apply the merge rule documented at the top of this module.
///
/// Returns the value that lost, when a declared precedence settled what
/// would otherwise have been a conflict.
fn merge_into(
    existing: &mut Property,
    incoming: Option<Value>,
    channel: Channel,
    order: Option<&Vec<Id>>,
) -> Option<Value> {
    use EpistemicStatus::{Conflicting, Inferred, Known};

    match (existing.status, channel) {
        // Conflict absorbs: once sources disagree, nothing later resolves
        // it. Resolution is evidence's job, with provenance to reason over.
        (Conflicting, _) => {}

        // An observation displaces anything short of a conflict.
        (Known, Channel::Observation) => {
            if existing.value != incoming {
                existing.status = Conflicting;
                existing.value = None;
            }
        }
        (_, Channel::Observation) => {
            existing.status = Known;
            existing.value = incoming;
        }

        // Inference cannot displace an observation.
        (Known, Channel::Inference) => {}

        (Inferred, Channel::Inference) => {
            if existing.value != incoming {
                // Precedence first, and only where the pack declared one.
                // Both values have to be in the order: a declared vocabulary
                // that does not mention one of them has said nothing about
                // how it compares, and guessing would be exactly the silent
                // decision this whole mechanism exists to avoid.
                match (
                    rank(order, existing.value.as_ref()),
                    rank(order, incoming.as_ref()),
                ) {
                    (Some(held), Some(arriving)) if held <= arriving => {
                        return incoming;
                    }
                    (Some(_), Some(_)) => {
                        let loser = existing.value.take();
                        existing.value = incoming;
                        return loser;
                    }
                    _ => {
                        existing.status = Conflicting;
                        existing.value = None;
                    }
                }
            }
        }
        (_, Channel::Inference) => {
            existing.status = Inferred;
            existing.value = incoming;
        }
    }

    None
}

#[cfg(test)]
mod precedence_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn build(
        order: Option<Vec<Id>>,
        first: &str,
        second: &str,
    ) -> (Option<Value>, EpistemicStatus) {
        let mut table: DetMap<Id, Vec<Id>> = DetMap::new();
        if let Some(order) = order {
            table.insert(id("gate.decision"), order);
        }
        let mut builder = StateBuilder::new(Limits::default()).with_precedence(table);
        builder.entity(id("gate"), id("gate")).expect("entity");
        builder
            .infer(&id("gate"), id("decision"), Value::Symbol(id(first)))
            .expect("first");
        builder
            .infer(&id("gate"), id("decision"), Value::Symbol(id(second)))
            .expect("second");
        let state = builder.build();
        let property = state
            .entities
            .get(&id("gate"))
            .expect("entity")
            .properties
            .get(&id("decision"))
            .expect("property")
            .clone();
        (property.value, property.status)
    }

    fn order() -> Option<Vec<Id>> {
        Some(vec![id("deny"), id("manual_review"), id("allow")])
    }

    #[test]
    fn a_declared_order_settles_a_policy_conflict_either_way_round() {
        // Dominance is a fact about the values, so the answer cannot depend
        // on which rule happened to fire first.
        assert_eq!(
            build(order(), "manual_review", "deny"),
            (Some(Value::Symbol(id("deny"))), EpistemicStatus::Inferred)
        );
        assert_eq!(
            build(order(), "deny", "manual_review"),
            (Some(Value::Symbol(id("deny"))), EpistemicStatus::Inferred)
        );
    }

    #[test]
    fn without_a_declared_order_it_still_conflicts() {
        // The behaviour every pack written before this had, and the one that
        // is right for a belief rather than a policy.
        assert_eq!(
            build(None, "deny", "manual_review"),
            (None, EpistemicStatus::Conflicting)
        );
    }

    #[test]
    fn a_value_the_order_does_not_mention_conflicts() {
        // A vocabulary that does not rank `quarantine` has said nothing about
        // how it compares, and inventing an answer there would be exactly the
        // silent decision this mechanism exists to avoid.
        assert_eq!(
            build(order(), "deny", "quarantine"),
            (None, EpistemicStatus::Conflicting)
        );
    }
}
