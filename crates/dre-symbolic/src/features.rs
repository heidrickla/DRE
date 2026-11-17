//! Derived features.
//!
//! `features.yaml` has been in the pack format since plan section 9 and was
//! parsed by nothing. Closing that was not a matter of tidying up an unused
//! file — the gap it left was the one thing packs could not express.
//!
//! # Why a rule cannot do this
//!
//! A rule concludes a *literal*: [`crate::Inference`] carries a
//! `Value`, not a computation. So a pack can say "if temperature exceeds
//! 800 then bearing_fault is true" and cannot say "efficiency is output
//! divided by input" at all. Any domain whose reasoning starts with a
//! derived quantity — a ratio, a margin, a difference from a set point —
//! had no way to state it without changing the engine.
//!
//! That collides directly with the claim in `model-packs/README.md`: adding
//! a reasoning domain must require a new pack and no change to the runtime.
//! A missing arithmetic layer made that false for a whole class of domains,
//! which is a finding about the architecture rather than a missing
//! convenience.
//!
//! # Three outcomes, not two
//!
//! A derivation can fail in two different ways and the difference matters:
//!
//! - An operand is not available — never observed, still `UNKNOWN`, or
//!   `CONFLICTING`. The feature is `UNKNOWN`. This is the three-valued
//!   discipline of [`crate::eval`] carried into arithmetic: computing from
//!   a substituted default would produce a number that looks exactly like
//!   a measured one.
//! - Every operand is available and the operation has no defined result —
//!   division by zero, or overflow. The feature is `INVALID`.
//!
//! Collapsing those two into `UNKNOWN` would lose a real distinction. "We
//! never measured the input power" and "the input power is zero, so
//! efficiency is not a number" are different states of the world, and only
//! the second one is a fact about the machine.
//!
//! Overflow is `INVALID` rather than a saturated value for the reason given
//! in `docs/decisions.md`: clamping makes a result depend on how close the
//! inputs were to the representable boundary, which is a discontinuity
//! nothing downstream can see.
//!
//! # Evaluation order
//!
//! Features may read other features. Rather than require the pack to
//! declare them in dependency order — which makes the file's *ordering*
//! semantic, and orderings get edited — they run to a fixpoint in
//! identifier order, exactly as rules do. A feature that cannot yet be
//! computed is retried next round.
//!
//! Features that never resolve are written once, at the end, with their
//! final verdict. Doing it that way rather than writing `UNKNOWN`
//! provisionally each round keeps the proof to one node per feature, and a
//! cycle simply leaves both members `UNKNOWN` instead of spinning.

use dre_core::collections::DetSet;
use dre_core::context::ExecutionContext;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_proof::{NodeKind, ProofNode, Trace};
use dre_state::{CanonicalState, EpistemicStatus, StateBuilder, Value};
use serde::{Deserialize, Serialize};

use crate::eval::split_subject;
use crate::Condition;

/// Maximum fixpoint rounds before giving up.
///
/// A backstop rather than a tuning knob, matching [`crate::eval`]. Each
/// round must compute at least one new feature or the loop ends, so this
/// can only be reached by a pack with more features than rounds.
pub const MAX_ROUNDS: u32 = 1_000;

/// An arithmetic expression over properties and constants.
///
/// Externally tagged, so a pack writes `{ divide: [...] }` and
/// `{ subject: pump.output_power }` — the shape a reader would guess.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expression {
    /// Read a property, as `entity.property`.
    Subject(Id),
    /// A constant.
    Literal(Fixed),
    /// Sum of the operands.
    Add(Vec<Expression>),
    /// First operand less each of the rest.
    Subtract(Vec<Expression>),
    /// Product of the operands.
    Multiply(Vec<Expression>),
    /// First operand divided by each of the rest.
    Divide(Vec<Expression>),
    /// Smallest operand.
    Min(Vec<Expression>),
    /// Largest operand.
    Max(Vec<Expression>),
    /// Magnitude of the operand.
    Abs(Box<Expression>),
    /// Sign-flipped operand.
    Negate(Box<Expression>),
}

/// A declared derived feature.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// Unique identifier. Also the evaluation order.
    pub id: Id,
    /// Property this writes, as `entity.property`.
    pub subject: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// How the value is computed.
    pub expression: Expression,
    /// Conditions that must hold for the feature to apply.
    ///
    /// A guard that does not hold makes the feature `NOT_APPLICABLE` —
    /// distinct from `UNKNOWN`, because the model is saying the quantity
    /// is meaningless here rather than unmeasured. A guard that cannot be
    /// decided leaves it `UNKNOWN`, since whether it applies is exactly
    /// what is not known.
    ///
    /// This is what a guard is *for*: without it a pack would have to
    /// express "no efficiency for a machine with no outlet" as arithmetic
    /// that happens to fail, and a failure is a much weaker statement than
    /// a declaration.
    #[serde(default)]
    pub when: Vec<Condition>,
}

/// The `features.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesSection {
    /// Declared features.
    #[serde(default)]
    pub features: Vec<Feature>,
}

/// What evaluating a feature produced.
///
/// Four outcomes, each mapping to a distinct epistemic status, because
/// they are four different statements about the world:
///
/// | Outcome | Status | Means |
/// |---|---|---|
/// | [`Value`](Self::Value) | `INFERRED` | derived, not measured |
/// | [`Unknown`](Self::Unknown) | `UNKNOWN` | an operand was not available |
/// | [`Invalid`](Self::Invalid) | `INVALID` | operands present, operation undefined |
/// | [`NotApplicable`](Self::NotApplicable) | `NOT_APPLICABLE` | the model says it does not apply here |
///
/// Collapsing any pair of these would lose something a caller needs. "We
/// never measured the input power", "the input power is zero so efficiency
/// is not a number", and "this pump has no efficiency to speak of" all
/// leave the property without a value and are not the same fact.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Derived {
    /// A value.
    Value(Fixed),
    /// An operand was not available.
    Unknown,
    /// Every operand was available and the operation has no result.
    Invalid(String),
    /// A guard says this quantity is meaningless for this entity.
    NotApplicable,
}

impl Derived {
    /// The epistemic status this outcome writes.
    #[must_use]
    pub const fn status(&self) -> EpistemicStatus {
        match self {
            Self::Value(_) => EpistemicStatus::Inferred,
            Self::Unknown => EpistemicStatus::Unknown,
            Self::Invalid(_) => EpistemicStatus::Invalid,
            Self::NotApplicable => EpistemicStatus::NotApplicable,
        }
    }
}

/// Errors from deriving features.
#[derive(Debug, thiserror::Error)]
pub enum FeatureError {
    /// The fixpoint loop exceeded [`MAX_ROUNDS`].
    #[error("feature derivation did not converge within {MAX_ROUNDS} rounds")]
    NoFixpoint,

    /// Writing a derived value to state failed.
    #[error("applying feature failed: {0}")]
    Apply(#[from] dre_state::StateError),

    /// A traversal in a guard could not be evaluated.
    #[error("features: {0}")]
    Eval(#[from] crate::EvalError),

    /// A feature referenced two entity types, which is a join.
    ///
    /// Refused rather than guessed at, exactly as it is for rules: choosing
    /// cross-product or pairwise silently would make a derived value depend
    /// on a decision nobody wrote down.
    #[error("feature {feature} references entity types {first} and {second}; a feature may bind to one type")]
    MultipleTypes {
        /// The feature.
        feature: Id,
        /// First type seen.
        first: Id,
        /// The second, which makes it a join.
        second: Id,
    },

    /// A feature named a subject that is not `entity.property`.
    #[error("feature {feature} writes {subject}, which is not of the form entity.property")]
    MalformedSubject {
        /// The offending feature.
        feature: Id,
        /// What it tried to write.
        subject: Id,
    },
}

/// Read a property as a number, if the state is confident in one.
///
/// `None` covers every reason a value might not be usable — absent,
/// `UNKNOWN`, `CONFLICTING`, `INVALID`, or a non-numeric type. They are
/// deliberately not distinguished here: from arithmetic's point of view
/// there is no operand, and inventing one is the failure this prevents.
fn read(state: &CanonicalState, subject: &Id) -> Option<Fixed> {
    let (entity_name, property_name) = split_subject(subject)?;

    // Falls back to the sole instance of a type, exactly as `eval::lookup`
    // does. Without it one feature declaration resolved subjects by two
    // different rules: its `when:` guard went through `evaluate` and got the
    // fallback, its `expression:` came here and did not.
    let property = state
        .resolve_entity(entity_name)?
        .properties
        .iter()
        .find(|(id, _)| id.as_str() == property_name)
        .map(|(_, property)| property)?;

    if !matches!(
        property.status,
        EpistemicStatus::Known | EpistemicStatus::Inferred
    ) {
        return None;
    }

    match property.value.as_ref()? {
        Value::Integer(value) => Fixed::from_int(*value).ok(),
        Value::Fixed(value) => Some(*value),
        _ => None,
    }
}

/// Every property an expression reads, in the order it names them.
#[must_use]
pub fn referenced_subjects(expression: &Expression) -> Vec<Id> {
    let mut out = Vec::new();
    collect(expression, &mut out);
    out
}

fn collect(expression: &Expression, out: &mut Vec<Id>) {
    match expression {
        Expression::Subject(subject) => out.push(subject.clone()),
        Expression::Literal(_) => {}
        Expression::Add(operands)
        | Expression::Subtract(operands)
        | Expression::Multiply(operands)
        | Expression::Divide(operands)
        | Expression::Min(operands)
        | Expression::Max(operands) => {
            for operand in operands {
                collect(operand, out);
            }
        }
        Expression::Abs(operand) | Expression::Negate(operand) => collect(operand, out),
    }
}

/// Evaluate an expression against state.
#[must_use]
pub fn evaluate(expression: &Expression, state: &CanonicalState) -> Derived {
    match expression {
        Expression::Subject(subject) => {
            read(state, subject).map_or(Derived::Unknown, Derived::Value)
        }
        Expression::Literal(value) => Derived::Value(*value),

        Expression::Add(operands) => fold(operands, state, "add", Fixed::ZERO, Fixed::checked_add),
        Expression::Multiply(operands) => {
            fold(operands, state, "multiply", Fixed::ONE, Fixed::checked_mul)
        }

        Expression::Subtract(operands) => {
            fold_left(operands, state, "subtract", Fixed::checked_sub)
        }
        Expression::Divide(operands) => fold_left(operands, state, "divide", Fixed::checked_div),

        // Min and max need no identity and cannot fail arithmetically, so
        // they fold with an infallible step rather than a checked one.
        Expression::Min(operands) => extremum(operands, state, "min", Fixed::min),
        Expression::Max(operands) => extremum(operands, state, "max", Fixed::max),

        Expression::Abs(operand) => unary(operand, state, "abs", Fixed::checked_abs),
        Expression::Negate(operand) => unary(operand, state, "negate", Fixed::checked_neg),
    }
}

/// Fold from an identity, so `add` of nothing is zero rather than an error.
fn fold(
    operands: &[Expression],
    state: &CanonicalState,
    name: &str,
    identity: Fixed,
    step: fn(Fixed, Fixed) -> dre_core::Result<Fixed>,
) -> Derived {
    let mut total = identity;

    for operand in operands {
        match evaluate(operand, state) {
            Derived::Value(value) => match step(total, value) {
                Ok(next) => total = next,
                Err(error) => return Derived::Invalid(format!("{name}: {error}")),
            },
            // One operand that cannot contribute makes the whole
            // expression unable to contribute, whatever the reason. There
            // is no partial answer to give, and the outcome is passed
            // through rather than flattened so it still names the cause.
            other => return other,
        }
    }

    Derived::Value(total)
}

/// Fold left from the first operand, for the non-commutative operations.
fn fold_left(
    operands: &[Expression],
    state: &CanonicalState,
    name: &str,
    step: fn(Fixed, Fixed) -> dre_core::Result<Fixed>,
) -> Derived {
    let Some((first, rest)) = operands.split_first() else {
        return Derived::Invalid(format!("{name} needs at least two operands, got none"));
    };
    if rest.is_empty() {
        return Derived::Invalid(format!("{name} needs at least two operands, got one"));
    }

    let mut total = match evaluate(first, state) {
        Derived::Value(value) => value,
        other => return other,
    };

    for operand in rest {
        match evaluate(operand, state) {
            Derived::Value(value) => match step(total, value) {
                Ok(next) => total = next,
                Err(error) => return Derived::Invalid(format!("{name}: {error}")),
            },
            other => return other,
        }
    }

    Derived::Value(total)
}

fn extremum(
    operands: &[Expression],
    state: &CanonicalState,
    name: &str,
    step: fn(Fixed, Fixed) -> Fixed,
) -> Derived {
    let Some((first, rest)) = operands.split_first() else {
        return Derived::Invalid(format!("{name} needs at least one operand, got none"));
    };

    let mut total = match evaluate(first, state) {
        Derived::Value(value) => value,
        other => return other,
    };

    for operand in rest {
        match evaluate(operand, state) {
            Derived::Value(value) => total = step(total, value),
            other => return other,
        }
    }

    Derived::Value(total)
}

fn unary(
    operand: &Expression,
    state: &CanonicalState,
    name: &str,
    step: fn(Fixed) -> dre_core::Result<Fixed>,
) -> Derived {
    match evaluate(operand, state) {
        Derived::Value(value) => match step(value) {
            Ok(result) => Derived::Value(result),
            Err(error) => Derived::Invalid(format!("{name}: {error}")),
        },
        other => other,
    }
}

/// Evaluate one feature, guard included.
///
/// # Errors
///
/// Fails when a guard contains a traversal that cannot be evaluated — an
/// unresolved anchor, or `the` over several targets. Both are refusals
/// rather than an undecidable guard, which is why they do not become
/// `Derived::Unknown`.
pub fn derive(feature: &Feature, state: &CanonicalState) -> Result<Derived, FeatureError> {
    if !feature.when.is_empty() {
        let mut holds = crate::Truth::True;
        for condition in &feature.when {
            holds = holds.and(crate::evaluate(condition, state)?);
        }

        match holds {
            crate::Truth::True => {}
            // The model says this quantity is meaningless here. Not an
            // error and not a gap -- a statement, and the one status that
            // says so.
            crate::Truth::False => return Ok(Derived::NotApplicable),
            // Whether it applies is exactly what is not known, so the
            // feature is unknown rather than inapplicable.
            crate::Truth::Unknown => return Ok(Derived::Unknown),
        }
    }

    Ok(evaluate(&feature.expression, state))
}

/// What one round of derivation produced.
struct Applied {
    state: CanonicalState,
    computed: bool,
}

/// Write one feature's outcome into state and record it in the proof.
fn apply(
    feature: &Feature,
    outcome: &Derived,
    state: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<Applied, FeatureError> {
    let (entity_name, property_name) =
        split_subject(&feature.subject).ok_or_else(|| FeatureError::MalformedSubject {
            feature: feature.id.clone(),
            subject: feature.subject.clone(),
        })?;

    let entity = Id::new(entity_name).map_err(|_| FeatureError::MalformedSubject {
        feature: feature.id.clone(),
        subject: feature.subject.clone(),
    })?;
    let property = Id::new(property_name).map_err(|_| FeatureError::MalformedSubject {
        feature: feature.id.clone(),
        subject: feature.subject.clone(),
    })?;

    let mut builder = StateBuilder::from_state(state, context.config().limits);

    match outcome {
        Derived::Value(value) => {
            // `infer`, not `set`: a derived value must not displace a
            // direct observation of the same property. If a pack both
            // measures and derives one quantity, the measurement wins and
            // the merge rule is where that is decided.
            builder.infer(&entity, property, Value::Fixed(*value))?;
        }
        Derived::Unknown | Derived::Invalid(_) | Derived::NotApplicable => {
            builder.declare(&entity, property, outcome.status())?;
        }
    }

    let inputs = trace.sources_of(referenced_subjects(&feature.expression).iter());
    trace.establish(
        feature.subject.clone(),
        ProofNode {
            kind: NodeKind::Feature {
                feature: feature.id.clone(),
                subject: feature.subject.clone(),
                resolved: matches!(outcome, Derived::Value(_)),
            },
            inputs,
        },
    );
    context.charge_proof_node();

    if matches!(outcome, Derived::Value(_)) {
        context.charge_reasoning_step();
    }

    Ok(Applied {
        state: builder.build(),
        computed: matches!(outcome, Derived::Value(_)),
    })
}

/// Rewrite every subject in an expression tree for one binding.
fn rebind_expression(expression: &Expression, type_id: &Id, instance: &Id) -> Expression {
    let rebind_all = |operands: &Vec<Expression>| -> Vec<Expression> {
        operands
            .iter()
            .map(|operand| rebind_expression(operand, type_id, instance))
            .collect()
    };
    match expression {
        Expression::Subject(subject) => {
            Expression::Subject(crate::eval::rebind_subject(subject, type_id, instance))
        }
        Expression::Literal(value) => Expression::Literal(*value),
        Expression::Add(operands) => Expression::Add(rebind_all(operands)),
        Expression::Subtract(operands) => Expression::Subtract(rebind_all(operands)),
        Expression::Multiply(operands) => Expression::Multiply(rebind_all(operands)),
        Expression::Divide(operands) => Expression::Divide(rebind_all(operands)),
        Expression::Min(operands) => Expression::Min(rebind_all(operands)),
        Expression::Max(operands) => Expression::Max(rebind_all(operands)),
        Expression::Abs(operand) => {
            Expression::Abs(Box::new(rebind_expression(operand, type_id, instance)))
        }
        Expression::Negate(operand) => {
            Expression::Negate(Box::new(rebind_expression(operand, type_id, instance)))
        }
    }
}

/// A feature rebound onto one instance.
fn rebind_feature(feature: &Feature, type_id: &Id, instance: &Id) -> Feature {
    let mut bound = feature.clone();
    bound.subject = crate::eval::rebind_subject(&feature.subject, type_id, instance);
    bound.expression = rebind_expression(&feature.expression, type_id, instance);
    bound.when = feature
        .when
        .iter()
        .map(|condition| crate::eval::rebind_condition(condition, type_id, instance))
        .collect();
    bound
}

/// Which instances a feature should be derived for.
///
/// The same rule rules follow, and for the same reason: a derived quantity
/// is as naturally per-instance as an inference is, and a pack that can say
/// "every host's patch age" in a rule but not in a feature is drawing a line
/// nobody can defend.
///
/// It also has to be the same rule, not merely a similar one. Three
/// independently authored packs — Windows administration, network
/// forwarding, Linux systemd — each wrote type-named feature subjects on
/// their first attempt, because the brief documents subjects once and
/// features are subjects. All three failed to run with `references unknown
/// entity`, which is a documentation promise the engine did not keep.
fn feature_bindings(
    feature: &Feature,
    state: &CanonicalState,
) -> Result<Vec<Option<Id>>, FeatureError> {
    let mut subjects = vec![feature.subject.clone()];
    subjects.extend(referenced_subjects(&feature.expression));
    subjects.extend(crate::eval::referenced_subjects(&feature.when));

    let mut named: Option<Id> = None;
    for subject in &subjects {
        let Some((entity_name, _)) = subject.as_str().rsplit_once('.') else {
            continue;
        };
        // An exact instance name wins over a type of the same name, exactly
        // as it does for rules.
        let names_an_instance = Id::new(entity_name)
            .ok()
            .is_some_and(|id| state.entities.contains_key(&id));
        if names_an_instance {
            continue;
        }
        let is_type = state
            .entities
            .values()
            .any(|entity| entity.type_id.as_str() == entity_name);
        if !is_type {
            continue;
        }
        let Ok(type_id) = Id::new(entity_name) else {
            continue;
        };
        match &named {
            None => named = Some(type_id),
            Some(first) if first == &type_id => {}
            Some(first) => {
                return Err(FeatureError::MultipleTypes {
                    feature: feature.id.clone(),
                    first: first.clone(),
                    second: type_id,
                })
            }
        }
    }

    let Some(type_id) = named else {
        return Ok(vec![None]);
    };

    Ok(state
        .entities
        .iter()
        .filter(|(_, entity)| entity.type_id == type_id)
        .map(|(id, _)| Some(id.clone()))
        .collect())
}

/// Derive every feature, to a fixpoint.
///
/// Features that resolve are applied as they resolve, so one feature can
/// feed another whatever order the pack lists them in. Features that never
/// resolve are written once at the end with their final verdict, which
/// keeps the proof to one node per feature and makes a dependency cycle
/// terminate quietly rather than spin.
pub fn derive_all(
    features: &[Feature],
    initial: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<CanonicalState, FeatureError> {
    // Sorted by identifier, so the evaluation sequence is a property of
    // the pack rather than of the order someone edited the file in.
    let mut ordered: Vec<&Feature> = features.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    let mut state = initial;
    let mut resolved: DetSet<Id> = DetSet::new();

    for _ in 0..MAX_ROUNDS {
        let mut progressed = false;

        for feature in &ordered {
            if resolved.contains(&feature.id) {
                continue;
            }

            // One binding per instance where the feature names a type, and
            // the single unbound case otherwise. A type-bound feature counts
            // as resolved only when every instance of it derived, so a fleet
            // where one member is missing a reading keeps the feature live
            // for the next round rather than declaring itself finished.
            let bindings = feature_bindings(feature, &state)?;
            let mut derived_all = true;

            for binding in bindings {
                let bound = match &binding {
                    Some(instance) => {
                        let type_id = crate::eval::type_of(&state, instance);
                        rebind_feature(feature, &type_id, instance)
                    }
                    None => (*feature).clone(),
                };

                let outcome = derive(&bound, &state)?;
                if !matches!(outcome, Derived::Value(_)) {
                    derived_all = false;
                    continue;
                }

                let applied = apply(&bound, &outcome, state, context, trace)?;
                state = applied.state;
                if !applied.computed {
                    derived_all = false;
                }
            }

            if derived_all {
                resolved.insert(feature.id.clone());
                progressed = true;
            }
        }

        if !progressed {
            // Nothing new resolved, so nothing more will. Whatever is left
            // is genuinely underivable from this state.
            let remaining: Vec<&Feature> = ordered
                .iter()
                .filter(|feature| !resolved.contains(&feature.id))
                .copied()
                .collect();

            return record_underivable(remaining, state, context, trace);
        }
    }

    Err(FeatureError::NoFixpoint)
}

/// Write the features that never resolved, dependencies first.
///
/// Order matters here for the proof rather than for the result: every one
/// of these writes `UNKNOWN` or `INVALID` whatever sequence they go in, but
/// a proof edge can only point at a node that already exists. Writing
/// `efficiency_percent` before `efficiency` leaves the interesting question
/// — *why* could it not be derived — with no edge to follow, when the
/// answer is sitting two nodes further down.
///
/// So each round writes only the features none of the remaining ones
/// supply an input to. A dependency cycle makes no progress, and the rest
/// are written in identifier order; the edges are then genuinely absent,
/// which is the truthful encoding of a cycle.
fn record_underivable(
    mut remaining: Vec<&Feature>,
    mut state: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<CanonicalState, FeatureError> {
    while !remaining.is_empty() {
        let pending: DetSet<Id> = remaining
            .iter()
            .map(|feature| feature.subject.clone())
            .collect();

        let (ready, blocked): (Vec<&Feature>, Vec<&Feature>) =
            remaining.iter().partition(|feature| {
                !referenced_subjects(&feature.expression)
                    .iter()
                    .any(|subject| pending.contains(subject) && *subject != feature.subject)
            });

        // A cycle: nothing is ready, so identifier order is all that is
        // left. Deterministic, and the missing edges say so.
        let batch = if ready.is_empty() {
            blocked.clone()
        } else {
            ready
        };

        for feature in &batch {
            // Rebound per instance here too, and forgetting it was a real
            // defect rather than a tidiness point.
            //
            // Engine 0.15.0 taught the SUCCESS path to bind a type-named
            // feature per instance and left this one writing the original.
            // So a feature that derived was fine and a feature that could
            // not derive raised `references unknown entity unit` -- the
            // exact error 0.15.0 was released to remove, still reachable
            // through the other door. `linux-admin` and
            // `network-forwarding-policy` were latent on it: both declare
            // type-named features and both ran only because every operand
            // in their committed experiments happens to be KNOWN. One
            // unobserved operand was enough to break either.
            //
            // The lesson is the same one this repository keeps paying for:
            // a rule applied on the path that works and not on the path
            // that fails is not applied.
            for binding in feature_bindings(feature, &state)? {
                let bound = match &binding {
                    Some(instance) => {
                        let type_id = crate::eval::type_of(&state, instance);
                        rebind_feature(feature, &type_id, instance)
                    }
                    None => (*feature).clone(),
                };
                let outcome = derive(&bound, &state)?;
                let applied = apply(&bound, &outcome, state, context, trace)?;
                state = applied.state;
            }
        }

        if batch.len() == remaining.len() {
            break;
        }
        remaining = blocked;
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn subject(name: &str) -> Expression {
        Expression::Subject(id(name))
    }

    fn literal(value: i64) -> Expression {
        Expression::Literal(Fixed::from_int(value).expect("in range"))
    }

    /// A pump with 800 W out of 1000 W in.
    fn pump() -> CanonicalState {
        let mut builder = StateBuilder::with_default_limits();
        builder.entity(id("pump"), id("pump")).expect("entity");
        builder
            .observe(&id("pump"), id("input_power"), Value::Integer(1_000))
            .expect("input");
        builder
            .observe(&id("pump"), id("output_power"), Value::Integer(800))
            .expect("output");
        builder
            .declare(&id("pump"), id("ambient"), EpistemicStatus::Unknown)
            .expect("ambient");
        builder.build()
    }

    fn value_of(outcome: &Derived) -> Fixed {
        match outcome {
            Derived::Value(value) => *value,
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn division_produces_a_fractional_result() {
        // The whole reason this layer exists: 800/1000 is 0.8, and a rule
        // concluding a literal cannot express it.
        let outcome = evaluate(
            &Expression::Divide(vec![
                subject("pump.output_power"),
                subject("pump.input_power"),
            ]),
            &pump(),
        );
        assert_eq!(value_of(&outcome), Fixed::from_raw(800_000));
    }

    #[test]
    fn subtraction_folds_left_rather_than_summing_the_tail() {
        // 1000 - 800 - 100 is 100, not 1000 - 900. Folding the wrong way
        // gives a plausible number for many inputs, which is what makes it
        // worth pinning.
        let outcome = evaluate(
            &Expression::Subtract(vec![
                subject("pump.input_power"),
                subject("pump.output_power"),
                literal(100),
            ]),
            &pump(),
        );
        assert_eq!(value_of(&outcome), Fixed::from_int(100).expect("in range"));
    }

    #[test]
    fn an_unavailable_operand_makes_the_whole_expression_unknown() {
        // Not zero, not a default. `ambient` is declared UNKNOWN, and any
        // substituted value would produce a number indistinguishable from
        // a measured one.
        let outcome = evaluate(
            &Expression::Add(vec![subject("pump.input_power"), subject("pump.ambient")]),
            &pump(),
        );
        assert_eq!(outcome, Derived::Unknown);
    }

    #[test]
    fn an_absent_property_is_unknown_rather_than_an_error() {
        let outcome = evaluate(&subject("pump.never_declared"), &pump());
        assert_eq!(outcome, Derived::Unknown);
    }

    #[test]
    fn division_by_zero_is_invalid_not_unknown() {
        // The distinction the two statuses exist to draw. Both operands are
        // known; the operation simply has no result. Reporting UNKNOWN
        // would claim the input was never measured.
        let outcome = evaluate(&Expression::Divide(vec![literal(1), literal(0)]), &pump());
        assert!(
            matches!(outcome, Derived::Invalid(_)),
            "expected INVALID, got {outcome:?}"
        );
        assert_eq!(outcome.status(), EpistemicStatus::Invalid);
    }

    #[test]
    fn overflow_is_invalid_rather_than_saturated() {
        // Clamping would make the result depend on how close the inputs
        // were to the representable boundary -- a discontinuity nothing
        // downstream could see.
        let outcome = evaluate(
            &Expression::Multiply(vec![
                Expression::Literal(Fixed::MAX),
                Expression::Literal(Fixed::MAX),
            ]),
            &pump(),
        );
        assert!(matches!(outcome, Derived::Invalid(_)), "got {outcome:?}");
    }

    #[test]
    fn an_unknown_operand_beats_an_invalid_sibling_only_when_it_comes_first() {
        // Evaluation is left to right and stops at the first operand that
        // cannot contribute. Worth stating explicitly, because it means the
        // reported reason depends on operand order -- and that order is
        // fixed by the pack, so the result is still deterministic.
        let unknown_first = evaluate(
            &Expression::Add(vec![
                subject("pump.ambient"),
                Expression::Divide(vec![literal(1), literal(0)]),
            ]),
            &pump(),
        );
        assert_eq!(unknown_first, Derived::Unknown);

        let invalid_first = evaluate(
            &Expression::Add(vec![
                Expression::Divide(vec![literal(1), literal(0)]),
                subject("pump.ambient"),
            ]),
            &pump(),
        );
        assert!(matches!(invalid_first, Derived::Invalid(_)));
    }

    #[test]
    fn arity_errors_are_reported_rather_than_guessed_at() {
        // `subtract` of one operand could plausibly mean negation. It does
        // not, and silently picking a meaning would make a typo compute
        // something.
        let outcome = evaluate(&Expression::Subtract(vec![literal(5)]), &pump());
        assert!(
            matches!(&outcome, Derived::Invalid(reason) if reason.contains("at least two")),
            "got {outcome:?}"
        );
    }

    #[test]
    fn min_and_max_need_no_identity() {
        assert_eq!(
            value_of(&evaluate(
                &Expression::Min(vec![literal(5), literal(2), literal(9)]),
                &pump()
            )),
            Fixed::from_int(2).expect("in range")
        );
        assert_eq!(
            value_of(&evaluate(
                &Expression::Max(vec![literal(5), literal(2), literal(9)]),
                &pump()
            )),
            Fixed::from_int(9).expect("in range")
        );
    }

    #[test]
    fn nested_expressions_evaluate_inside_out() {
        // efficiency as a percentage: (output / input) * 100
        let outcome = evaluate(
            &Expression::Multiply(vec![
                Expression::Divide(vec![
                    subject("pump.output_power"),
                    subject("pump.input_power"),
                ]),
                literal(100),
            ]),
            &pump(),
        );
        assert_eq!(value_of(&outcome), Fixed::from_int(80).expect("in range"));
    }

    fn feature(name: &str, writes: &str, expression: Expression) -> Feature {
        Feature {
            id: id(name),
            subject: id(writes),
            description: String::new(),
            expression,
            when: Vec::new(),
        }
    }

    fn derive_into(features: &[Feature]) -> (CanonicalState, dre_proof::ProofGraph) {
        let mut context = ExecutionContext::with_defaults().expect("context");
        let mut trace = Trace::new();
        let state = derive_all(features, pump(), &mut context, &mut trace).expect("converges");
        (state, trace.into_graph())
    }

    fn status_of(state: &CanonicalState, entity: &str, property: &str) -> EpistemicStatus {
        state
            .entities
            .get(&id(entity))
            .expect("entity")
            .properties
            .get(&id(property))
            .expect("property")
            .status
    }

    #[test]
    fn a_derived_value_lands_as_inferred() {
        let (state, _) = derive_into(&[feature(
            "F001",
            "pump.efficiency",
            Expression::Divide(vec![
                subject("pump.output_power"),
                subject("pump.input_power"),
            ]),
        )]);

        assert_eq!(
            status_of(&state, "pump", "efficiency"),
            EpistemicStatus::Inferred,
            "derived, not observed -- the distinction the engine exists to keep"
        );
    }

    #[test]
    fn a_feature_may_read_a_feature_declared_after_it() {
        // F001 depends on F002. Requiring dependency order in the file
        // would make the ordering semantic, and orderings get edited.
        let features = vec![
            feature(
                "F001",
                "pump.loss_percent",
                Expression::Multiply(vec![subject("pump.loss_ratio"), literal(100)]),
            ),
            feature(
                "F002",
                "pump.loss_ratio",
                Expression::Subtract(vec![
                    literal(1),
                    Expression::Divide(vec![
                        subject("pump.output_power"),
                        subject("pump.input_power"),
                    ]),
                ]),
            ),
        ];

        let (state, _) = derive_into(&features);

        let percent = state
            .entities
            .get(&id("pump"))
            .expect("pump")
            .properties
            .get(&id("loss_percent"))
            .expect("loss_percent")
            .value
            .clone();

        assert_eq!(
            percent,
            Some(Value::Fixed(Fixed::from_int(20).expect("ok")))
        );
    }

    #[test]
    fn a_dependency_cycle_terminates_with_both_unknown() {
        // Neither can ever resolve. The loop must end quietly rather than
        // run to the round cap and report a failure the pack cannot act on.
        let features = vec![
            feature("F001", "pump.a", subject("pump.b")),
            feature("F002", "pump.b", subject("pump.a")),
        ];

        let (state, proof) = derive_into(&features);

        assert_eq!(status_of(&state, "pump", "a"), EpistemicStatus::Unknown);
        assert_eq!(status_of(&state, "pump", "b"), EpistemicStatus::Unknown);
        assert_eq!(proof.len(), 2, "one node per feature, not one per round");
    }

    #[test]
    fn a_feature_does_not_displace_an_observation() {
        // If a pack both measures and derives one quantity, the
        // measurement wins. Deciding that here rather than in the merge
        // rule would give features a privilege rules do not have.
        let (state, _) = derive_into(&[feature(
            "F001",
            "pump.output_power",
            Expression::Literal(Fixed::from_int(1).expect("in range")),
        )]);

        assert_eq!(
            status_of(&state, "pump", "output_power"),
            EpistemicStatus::Known
        );
    }

    #[test]
    fn a_guard_that_fails_makes_the_feature_not_applicable() {
        use crate::Operator;

        // Three ways to have no value, and this is the third. Not UNKNOWN
        // (nothing is missing) and not INVALID (no arithmetic was even
        // attempted) -- the model declared the quantity meaningless here.
        let mut guarded = feature(
            "F001",
            "pump.efficiency",
            Expression::Divide(vec![
                subject("pump.output_power"),
                subject("pump.input_power"),
            ]),
        );
        guarded.when = vec![Condition::Compare {
            subject: id("pump.input_power"),
            operator: Operator::Gt,
            operand: Value::Integer(5_000),
        }];

        let (state, _) = derive_into(&[guarded]);
        assert_eq!(
            status_of(&state, "pump", "efficiency"),
            EpistemicStatus::NotApplicable,
            "the model said it does not apply, which is a statement not a gap"
        );
    }

    #[test]
    fn the_four_outcomes_map_to_four_distinct_statuses() {
        // Guards the mapping itself. Collapsing any pair would make two
        // different facts about the world indistinguishable downstream,
        // and every one of them leaves the property without a value -- so
        // nothing else would notice.
        let statuses = [
            Derived::Value(Fixed::ONE).status(),
            Derived::Unknown.status(),
            Derived::Invalid(String::new()).status(),
            Derived::NotApplicable.status(),
        ];

        let distinct: DetSet<EpistemicStatus> = statuses.into_iter().collect();
        assert_eq!(distinct.len(), 4, "two outcomes share a status");
    }

    #[test]
    fn an_undecidable_guard_leaves_the_feature_unknown() {
        use crate::Operator;

        let mut guarded = feature("F001", "pump.efficiency", literal(1));
        guarded.when = vec![Condition::Compare {
            subject: id("pump.ambient"),
            operator: Operator::Gt,
            operand: Value::Integer(0),
        }];

        let (state, _) = derive_into(&[guarded]);
        assert_eq!(
            status_of(&state, "pump", "efficiency"),
            EpistemicStatus::Unknown,
            "whether it applies is exactly what is not known"
        );
    }

    #[test]
    fn declaration_order_does_not_reach_the_result() {
        use dre_core::canonical::Canonical;

        let first = feature(
            "F001",
            "pump.loss",
            Expression::Subtract(vec![
                subject("pump.input_power"),
                subject("pump.output_power"),
            ]),
        );
        let second = feature(
            "F002",
            "pump.efficiency",
            Expression::Divide(vec![
                subject("pump.output_power"),
                subject("pump.input_power"),
            ]),
        );

        let (forward, forward_proof) = derive_into(&[first.clone(), second.clone()]);
        let (backward, backward_proof) = derive_into(&[second, first]);

        assert_eq!(forward.canonical_hash(), backward.canonical_hash());
        assert_eq!(forward_proof.proof_hash(), backward_proof.proof_hash());
    }

    #[test]
    fn a_failed_feature_points_at_the_one_that_failed_under_it() {
        // The question a failed derivation prompts is not "did it fail"
        // but "why", and the answer is the feature below it. Written in
        // identifier order, F001 would be recorded before the F002 it
        // depends on and there would be no node for its edge to reach.
        let features = vec![
            feature(
                "F001",
                "pump.efficiency_percent",
                Expression::Multiply(vec![subject("pump.efficiency"), literal(100)]),
            ),
            feature(
                "F002",
                "pump.efficiency",
                Expression::Divide(vec![subject("pump.output_power"), literal(0)]),
            ),
        ];

        let (state, proof) = derive_into(&features);

        assert_eq!(
            status_of(&state, "pump", "efficiency"),
            EpistemicStatus::Invalid
        );
        assert_eq!(
            status_of(&state, "pump", "efficiency_percent"),
            EpistemicStatus::Unknown,
            "invalidity is not contagious: the operand was simply unavailable"
        );

        let position = |wanted: &str| {
            proof
                .nodes()
                .iter()
                .position(|node| {
                    matches!(&node.kind, NodeKind::Feature { feature, .. }
                        if feature.as_str() == wanted)
                })
                .unwrap_or_else(|| panic!("no node for {wanted}"))
        };

        let root = position("F002");
        let dependent = position("F001");
        assert!(
            root < dependent,
            "the cause must be recorded before the effect"
        );
        assert_eq!(
            proof.nodes()[dependent].inputs,
            vec![dre_proof::NodeRef(root as u32)],
            "the failed feature must point at the failure underneath it"
        );
    }

    #[test]
    fn the_proof_records_what_each_feature_read() {
        let features = vec![feature(
            "F001",
            "pump.efficiency",
            Expression::Divide(vec![
                subject("pump.output_power"),
                subject("pump.input_power"),
            ]),
        )];

        let (_, proof) = derive_into(&features);
        let node = &proof.nodes()[0];

        assert!(matches!(
            &node.kind,
            NodeKind::Feature { feature, resolved: true, .. } if feature.as_str() == "F001"
        ));
        // Nothing established the operands in this harness, so there are no
        // edges to draw -- and inventing them would assert a dependency the
        // run never had. The runner's input nodes are what populate these.
        assert!(node.inputs.is_empty());
    }

    #[test]
    fn subjects_are_collected_from_the_whole_tree() {
        let expression = Expression::Add(vec![
            Expression::Abs(Box::new(subject("a.x"))),
            Expression::Divide(vec![subject("b.y"), literal(2)]),
        ]);

        let found: Vec<String> = referenced_subjects(&expression)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found, ["a.x", "b.y"]);
    }
}

#[cfg(test)]
mod type_bound_failure_path_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use dre_core::config::Limits;
    use dre_core::context::ExecutionContext;
    use dre_proof::Trace;
    use dre_state::{EpistemicStatus, StateBuilder};

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// A type-named feature that CANNOT derive must still write against the
    /// instance.
    ///
    /// Engine 0.15.0 taught the success path to bind per instance and left
    /// the failure path writing the original, so a feature that derived was
    /// fine and one that could not raised `references unknown entity unit` --
    /// the exact error 0.15.0 was released to remove, reachable through the
    /// other door. `linux-admin` and `network-forwarding-policy` were latent
    /// on it, running only because every operand in their committed
    /// experiments happens to be KNOWN.
    ///
    /// The general lesson, which is why this test exists rather than a fixed
    /// experiment: a rule applied on the path that works and not on the path
    /// that fails is not applied.
    #[test]
    fn an_underivable_type_named_feature_writes_against_the_instance() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("web"), id("unit")).expect("entity");
        // Declared and never observed, so the subtraction has no operand.
        builder
            .declare(&id("web"), id("boot_tick"), EpistemicStatus::Unknown)
            .expect("declare");
        builder
            .declare(&id("web"), id("failure_tick"), EpistemicStatus::Unknown)
            .expect("declare");
        let state = builder.build();

        let feature = Feature {
            id: id("F001"),
            subject: id("unit.last_failure_age"),
            description: String::new(),
            expression: Expression::Subtract(vec![
                Expression::Subject(id("unit.boot_tick")),
                Expression::Subject(id("unit.failure_tick")),
            ]),
            when: Vec::new(),
        };

        let mut context = ExecutionContext::with_defaults().expect("context");
        let mut trace = Trace::new();
        let derived = derive_all(
            std::slice::from_ref(&feature),
            state,
            &mut context,
            &mut trace,
        )
        .expect("an underivable feature is not an error");

        let property = derived
            .entities
            .get(&id("web"))
            .expect("web is still in the state")
            .properties
            .get(&id("last_failure_age"))
            .expect("written against the instance, not the type");
        assert_eq!(property.status, EpistemicStatus::Unknown);
        assert!(property.value.is_none());

        // And nothing was written against the type name.
        assert!(!derived.entities.contains_key(&id("unit")));
    }
}
