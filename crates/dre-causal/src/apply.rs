//! Applying actions to state.

use dre_core::config::Limits;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_state::{CanonicalState, EpistemicStatus, StateBuilder, Value};
use dre_symbolic::{evaluate, Truth};

use crate::{Action, Effect};

/// Failures applying an action.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// An effect named a subject that is not `entity.property`.
    #[error("effect subject {subject} is not of the form entity.property")]
    MalformedSubject {
        /// The offending subject.
        subject: Id,
    },

    /// `Add` was applied to a property with no known numeric value.
    ///
    /// Deliberately an error. An `add: -200` against a temperature that
    /// was never observed has no defined result, and producing one anyway
    /// would invent a measurement.
    #[error("action {action} adds to {subject}, which has no known numeric value")]
    NotNumeric {
        /// The action.
        action: Id,
        /// The property.
        subject: Id,
    },

    /// Arithmetic overflowed.
    #[error("action {action} overflowed applying to {subject}")]
    Overflow {
        /// The action.
        action: Id,
        /// The property.
        subject: Id,
    },

    /// A traversal in a precondition could not be evaluated.
    #[error("precondition: {0}")]
    Eval(#[from] dre_symbolic::EvalError),

    /// Rebuilding state failed.
    #[error(transparent)]
    State(#[from] dre_state::StateError),
}

/// Whether every precondition holds.
///
/// Requires definite truth: a precondition that cannot be decided means
/// the action is not known to be applicable, and applying it anyway would
/// plan around a fact nobody has.
/// # Errors
///
/// Fails when a precondition contains a traversal that cannot be evaluated.
pub fn applicable(action: &Action, state: &CanonicalState) -> Result<bool, ApplyError> {
    let mut holds = Truth::True;
    for condition in &action.preconditions {
        holds = holds.and(evaluate(condition, state)?);
    }
    Ok(holds.is_true())
}

/// Apply an action's effects, producing the successor state.
pub fn apply(
    action: &Action,
    state: CanonicalState,
    limits: Limits,
) -> Result<CanonicalState, ApplyError> {
    let mut current = state;

    for (subject, effect) in &action.effects {
        let (entity_name, property_name) =
            subject
                .as_str()
                .rsplit_once('.')
                .ok_or_else(|| ApplyError::MalformedSubject {
                    subject: subject.clone(),
                })?;
        let entity = Id::new(entity_name)
            .map(|name| current.resolve_entity_id(name))
            .map_err(|_| ApplyError::MalformedSubject {
                subject: subject.clone(),
            })?;
        let property = Id::new(property_name).map_err(|_| ApplyError::MalformedSubject {
            subject: subject.clone(),
        })?;

        let next = match effect {
            Effect::Set(value) => Some(value.clone()),
            Effect::Clear => None,
            Effect::Add(delta) => Some(add_to(
                action, &current, &entity, &property, subject, *delta,
            )?),
        };

        // `set`, not `observe`. An effect supersedes the prior value
        // rather than offering a competing opinion about it -- see the
        // note on `StateBuilder::set`.
        let mut builder = StateBuilder::from_state(current, limits);
        let status = if next.is_some() {
            EpistemicStatus::Known
        } else {
            EpistemicStatus::Unknown
        };
        builder.set(&entity, property, next, status)?;
        current = builder.build();
    }

    Ok(current)
}

/// Compute `property + delta`, preserving the property's numeric kind.
fn add_to(
    action: &Action,
    state: &CanonicalState,
    entity: &Id,
    property: &Id,
    subject: &Id,
    delta: Fixed,
) -> Result<Value, ApplyError> {
    let not_numeric = || ApplyError::NotNumeric {
        action: action.id.clone(),
        subject: subject.clone(),
    };
    let overflow = || ApplyError::Overflow {
        action: action.id.clone(),
        subject: subject.clone(),
    };

    let existing = state
        .entities
        .get(entity)
        .and_then(|entity| entity.properties.get(property))
        .ok_or_else(not_numeric)?;

    if !matches!(
        existing.status,
        EpistemicStatus::Known | EpistemicStatus::Inferred
    ) {
        return Err(not_numeric());
    }

    match existing.value.as_ref().ok_or_else(not_numeric)? {
        Value::Integer(current) => {
            let base = Fixed::from_int(*current).map_err(|_| overflow())?;
            let sum = base.checked_add(delta).map_err(|_| overflow())?;
            Ok(Value::Integer(sum.to_int_trunc()))
        }
        Value::Fixed(current) => {
            let sum = current.checked_add(delta).map_err(|_| overflow())?;
            Ok(Value::Fixed(sum))
        }
        _ => Err(not_numeric()),
    }
}

/// The `causal.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CausalSection {
    /// Declared actions.
    #[serde(default)]
    pub actions: Vec<Action>,
}
