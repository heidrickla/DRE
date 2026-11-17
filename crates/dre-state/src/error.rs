//! Errors raised while building canonical state.

use dre_core::id::Id;
use thiserror::Error;

/// Failures in state construction.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StateError {
    /// An entity was declared twice.
    #[error("entity {entity} declared more than once")]
    DuplicateEntity {
        /// The repeated identifier.
        entity: Id,
    },

    /// A property was attached to an entity that was never declared.
    #[error("property {property} references unknown entity {entity}")]
    UnknownEntity {
        /// The missing entity.
        entity: Id,
        /// The property that referenced it.
        property: Id,
    },

    /// A relationship referenced an entity that was never declared.
    #[error("relationship {relationship} references unknown entity {entity}")]
    UnknownRelationshipEndpoint {
        /// The relationship.
        relationship: Id,
        /// The missing entity.
        entity: Id,
    },

    /// An endpoint's entity type is not what the relationship declares.
    #[error(
        "relationship {relationship} runs {side} {entity}, which is {actual}, but {relationship} is declared {side} {declared}"
    )]
    RelationshipEndpointType {
        /// The edge being declared.
        relationship: Id,
        /// The endpoint whose type is wrong.
        entity: Id,
        /// Which side it is on.
        side: &'static str,
        /// The type the ontology declares.
        declared: Id,
        /// The type the entity actually has.
        actual: Id,
    },

    /// A declared cardinality was violated by a second edge.
    ///
    /// `targets_per_source: one` means a `from` entity has at most one edge
    /// of that type. Enforced here rather than described in prose, so a
    /// functional edge is functional in the state — which is what lets a
    /// traversal over it produce one answer rather than a set that has to be
    /// collapsed.
    #[error("relationship type {relationship_type} declares {side}, and {entity} already has one")]
    ArityViolated {
        /// The relationship type whose cardinality was exceeded.
        relationship_type: Id,
        /// The entity that already had an edge.
        entity: Id,
        /// Which side was declared `one`.
        side: &'static str,
    },

    /// A relationship was declared twice.
    #[error("relationship {relationship} declared more than once")]
    DuplicateRelationship {
        /// The repeated identifier.
        relationship: Id,
    },

    /// A configured ceiling was exceeded.
    #[error("{limit} exceeded: {cap}")]
    LimitExceeded {
        /// Which ceiling.
        limit: &'static str,
        /// The ceiling value.
        cap: u64,
    },
}

/// Result alias for state construction.
pub type Result<T> = std::result::Result<T, StateError>;
