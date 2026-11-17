//! Condition evaluation and forward chaining.
//!
//! # Three-valued logic
//!
//! A condition does not evaluate to true or false. It evaluates to
//! [`Truth`], which has a third case: `Unknown`, meaning the state does
//! not contain enough information to decide.
//!
//! This is the crate's most consequential design choice, and it follows
//! directly from the MVP's objective of distinguishing known, unknown,
//! inferred and conflicting facts. A two-valued engine has to map "we have
//! not measured the temperature" onto either true or false, and both are
//! lies that propagate silently. With three values, a rule whose condition
//! is undecidable simply does not fire, and the proof says why.
//!
//! Negation is the case that shows the difference: `NOT unknown` is
//! `unknown`, not `true`. An engine that returned `true` there would
//! conclude things from its own ignorance.
//!
//! # Inferences apply immediately
//!
//! When a rule fires, its conclusions enter the state at once, so a rule
//! later in the firing order sees them within the same round. The
//! alternative — collecting a round's inferences and applying them
//! together — would make a chain of length *n* take *n* rounds instead of
//! one.
//!
//! Both are deterministic, because the firing order is total either way.
//! The difference is only how many rounds convergence takes, and immediate
//! application is the classical choice. It does mean rule order has
//! semantic weight beyond tie-breaking: a rule placed earlier can enable
//! one placed later in the same pass, which is exactly what makes the
//! `A → B → C` chain resolve in a single round.
//!
//! # Termination
//!
//! Forward chaining runs to a fixpoint: repeat until no rule adds anything
//! new. That terminates because the merge rule in `dre-state` is
//! monotone in practice — a property moves `UNKNOWN → INFERRED → KNOWN` or
//! to `CONFLICTING`, and `CONFLICTING` absorbs — so state cannot cycle.
//! An explicit round cap backs that reasoning up rather than trusting it,
//! because a termination argument that is only in a comment is one
//! refactor from being false.

use dre_core::collections::DetSet;
use dre_core::context::ExecutionContext;
use dre_core::id::Id;
use dre_proof::{NodeKind, ProofNode, Trace};
use dre_state::{CanonicalState, EpistemicStatus, StateBuilder, Value};

use crate::{Collapse, Condition, Direction, Operator, Reach, Rule};

/// Maximum forward-chaining rounds before declaring non-termination.
///
/// A backstop, not a tuning knob. Hitting it means the monotonicity
/// argument above is wrong, which is a bug rather than a big model.
pub const MAX_ROUNDS: u32 = 1_000;

/// The result of evaluating a condition.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Truth {
    /// Definitely holds.
    True,
    /// Definitely does not hold.
    False,
    /// Not decidable from the current state.
    Unknown,
}

impl Truth {
    /// Kleene conjunction: `false` wins over `unknown`.
    ///
    /// One false conjunct settles the whole conjunction regardless of what
    /// else is unknown, so this is stronger than short-circuiting on the
    /// first non-true.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::True, Self::True) => Self::True,
        }
    }

    /// Kleene disjunction: `true` wins over `unknown`.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::False, Self::False) => Self::False,
        }
    }

    /// Kleene negation. `NOT unknown` is `unknown`.
    ///
    /// Named `negate` rather than `not` deliberately: `!` would suggest
    /// two-valued semantics, and the whole point is that this is not.
    #[must_use]
    pub fn negate(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    /// Whether this is definitely true.
    #[must_use]
    pub const fn is_true(self) -> bool {
        matches!(self, Self::True)
    }
}

/// A dotted property reference such as `machine.temperature`.
///
/// Split at the **last** dot, so entity identifiers may themselves contain
/// dots while the property name may not.
pub(crate) fn split_subject(subject: &Id) -> Option<(&str, &str)> {
    subject.as_str().rsplit_once('.')
}

/// Look up a property, returning its value and status.
fn lookup<'a>(
    state: &'a CanonicalState,
    subject: &Id,
) -> Option<(Option<&'a Value>, EpistemicStatus)> {
    let (entity_name, property_name) = split_subject(subject)?;
    let entity = state.resolve_entity(entity_name)?;
    let property = entity
        .properties
        .iter()
        .find(|(id, _)| id.as_str() == property_name)
        .map(|(_, property)| property)?;
    Some((property.value.as_ref(), property.status))
}

/// Every traversal in a condition tree, with the collapse it declared.
fn collect_reaches<'a>(conditions: &'a [Condition], out: &mut Vec<(&'a Reach, &'a Collapse)>) {
    fn walk<'a>(condition: &'a Condition, out: &mut Vec<(&'a Reach, &'a Collapse)>) {
        match condition {
            Condition::Reach {
                reach, collapse, ..
            } => out.push((reach, collapse)),
            Condition::All { all } => {
                for inner in all {
                    walk(inner, out);
                }
            }
            Condition::Any { any } => {
                for inner in any {
                    walk(inner, out);
                }
            }
            Condition::Not { not } => walk(not, out),
            Condition::Compare { .. } | Condition::Status { .. } => {}
        }
    }
    for condition in conditions {
        walk(condition, out);
    }
}

/// The entities a traversal arrives at, and the anchor it started from.
///
/// Deduplicated, and in canonical entity order rather than in the order the
/// edges happened to be declared — two experiments listing the same edges
/// differently must reach the same answer.
pub(crate) fn reach_targets(reach: &Reach, state: &CanonicalState) -> Result<Vec<Id>, EvalError> {
    // The anchor must resolve. This is not a convenience check.
    //
    // `every` over an empty set is TRUE, so an anchor that silently resolved
    // to nothing would make every universal reach hold vacuously — a rule
    // firing on the strength of a name that reached nothing, with a proof
    // that looks entirely well formed. "The anchor resolved and has no edges
    // of this type" and "the anchor did not resolve" are different results
    // and must not share a representation.
    let anchor = state.resolve_entity(reach.from.as_str()).ok_or_else(|| {
        EvalError::UnresolvedReachAnchor {
            anchor: reach.from.clone(),
        }
    })?;

    let mut frontier: Vec<Id> = vec![anchor.id.clone()];

    for hop in &reach.path {
        let mut next: DetSet<Id> = DetSet::new();
        for current in &frontier {
            for edge in state.relationships.values() {
                if edge.type_id != hop.edge {
                    continue;
                }
                let (near, far) = match hop.direction {
                    Direction::Out => (&edge.from, &edge.to),
                    Direction::In => (&edge.to, &edge.from),
                };
                if near == current {
                    next.insert(far.clone());
                }
            }
        }
        // A `DetSet` iterates in key order, so the frontier is canonical at
        // every hop and cannot inherit declaration order.
        frontier = next.into_iter().collect();
        if frontier.is_empty() {
            break;
        }
    }

    Ok(frontier)
}

/// Evaluate a condition against state.
///
/// # Errors
///
/// Fails when a traversal cannot be evaluated at all — an anchor that
/// resolves to nothing, or a `the` collapse over several targets. Both are
/// refusals rather than answers: an undecidable comparison is `Unknown`, but
/// a traversal whose meaning is not determined has no truth value to report.
pub fn evaluate(condition: &Condition, state: &CanonicalState) -> Result<Truth, EvalError> {
    Ok(match condition {
        Condition::Compare {
            subject,
            operator,
            operand,
        } => compare(state, subject, *operator, operand),

        Condition::Status { subject, operator } => status(state, subject, *operator),

        Condition::All { all } => {
            let mut acc = Truth::True;
            for inner in all {
                acc = acc.and(evaluate(inner, state)?);
            }
            acc
        }

        Condition::Any { any } => {
            let mut acc = Truth::False;
            for inner in any {
                acc = acc.or(evaluate(inner, state)?);
            }
            acc
        }

        Condition::Not { not } => evaluate(not, state)?.negate(),

        Condition::Reach {
            reach,
            collapse,
            operator,
            operand,
        } => {
            let targets = reach_targets(reach, state)?;

            if *collapse == Collapse::The && targets.len() != 1 {
                // `the` is a claim that the traversal is functional. When it
                // is not, the pack asserted something false about its own
                // topology, and picking one would be the silent decision this
                // whole mechanism exists to prevent.
                return Err(EvalError::AmbiguousReach {
                    anchor: reach.from.clone(),
                    read: reach.read.clone(),
                    found: targets.len(),
                });
            }

            // Every test this reached entity has to satisfy: the main
            // comparison, then anything in `also`. Conjoined per entity and
            // only then handed to the collapse, so `some` means one entity
            // satisfied all of it rather than the set satisfying each part
            // somewhere.
            let judge = |target: &Id| -> Truth {
                let one = |property: &Id, operator: Operator, operand: Option<&Value>| -> Truth {
                    let Ok(subject) = Id::new(format!("{target}.{property}")) else {
                        return Truth::Unknown;
                    };
                    match operand {
                        Some(operand) => compare(state, &subject, operator, operand),
                        None => status(state, &subject, operator),
                    }
                };

                reach.also.iter().fold(
                    one(&reach.read, *operator, operand.as_ref()),
                    |acc, test| acc.and(one(&test.read, test.operator, test.operand.as_ref())),
                )
            };

            match collapse {
                // Exactly one target, checked above.
                Collapse::The => targets.first().map_or(Truth::Unknown, &judge),
                // Vacuously true over none, which is why the anchor check
                // above is an error and not an empty set.
                Collapse::Every => targets
                    .iter()
                    .fold(Truth::True, |acc, target| acc.and(judge(target))),
                Collapse::Some => targets
                    .iter()
                    .fold(Truth::False, |acc, target| acc.or(judge(target))),
                Collapse::AtLeast(wanted) => {
                    // Three-valued counting: a target that cannot be decided
                    // is neither a hit nor a miss, so the answer is Unknown
                    // whenever the undecided ones could still change it.
                    let mut definite = 0u32;
                    let mut possible = 0u32;
                    for target in &targets {
                        match judge(target) {
                            Truth::True => {
                                definite = definite.saturating_add(1);
                                possible = possible.saturating_add(1);
                            }
                            Truth::Unknown => possible = possible.saturating_add(1),
                            Truth::False => {}
                        }
                    }
                    if definite >= *wanted {
                        Truth::True
                    } else if possible < *wanted {
                        Truth::False
                    } else {
                        Truth::Unknown
                    }
                }
            }
        }
    })
}

/// Every property a set of conditions reads, in the order they name them.
///
/// Used to wire proof provenance: a rule's evaluation depends on whatever
/// established the properties it looked at. Duplicates are left in — the
/// caller deduplicates when resolving them to nodes, and keeping them here
/// means this stays a faithful reading of the condition tree rather than a
/// summary of it.
#[must_use]
pub fn referenced_subjects(conditions: &[Condition]) -> Vec<Id> {
    let mut out = Vec::new();
    for condition in conditions {
        collect_subjects(condition, &mut out);
    }
    out
}

fn collect_subjects(condition: &Condition, out: &mut Vec<Id>) {
    match condition {
        Condition::Compare { subject, .. } | Condition::Status { subject, .. } => {
            out.push(subject.clone());
        }
        Condition::All { all } => {
            for inner in all {
                collect_subjects(inner, out);
            }
        }
        Condition::Any { any } => {
            for inner in any {
                collect_subjects(inner, out);
            }
        }
        // A negated condition still *reads* its subject. Omitting it here
        // would make `NOT x` look like a rule that depended on nothing.
        Condition::Not { not } => collect_subjects(not, out),
        // A reach reads a property on entities it has not identified yet, so
        // there is no dotted subject to collect here. Its provenance is
        // resolved at evaluation, where the targets are known — see
        // `reach_targets`. Returning nothing is correct rather than lazy: a
        // fabricated subject would put an edge in the proof that no step
        // actually read.
        Condition::Reach { .. } => {}
    }
}

fn compare(state: &CanonicalState, subject: &Id, operator: Operator, operand: &Value) -> Truth {
    let Some((value, status)) = lookup(state, subject) else {
        return Truth::Unknown;
    };

    // A conflicting property answers nothing. Comparing against it would
    // let one of the disagreeing sources win by accident.
    if matches!(
        status,
        EpistemicStatus::Conflicting
            | EpistemicStatus::Unknown
            | EpistemicStatus::Unobservable
            | EpistemicStatus::Invalid
            | EpistemicStatus::NotApplicable
    ) {
        return Truth::Unknown;
    }

    let Some(value) = value else {
        return Truth::Unknown;
    };

    match operator {
        Operator::Eq => Truth::from(value == operand),
        Operator::Ne => Truth::from(value != operand),
        Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte => {
            ordered_compare(value, operator, operand)
        }
        // Not comparison operators; a pack using one here is malformed,
        // and undecidable is the safe reading.
        _ => Truth::Unknown,
    }
}

/// Ordered comparison, defined only between like kinds.
///
/// Comparing an integer against a symbol is not false, it is meaningless —
/// so it is `Unknown`. Returning `false` would let `NOT` turn a type error
/// into a firing rule.
fn ordered_compare(value: &Value, operator: Operator, operand: &Value) -> Truth {
    let ordering = match (value, operand) {
        (Value::Integer(left), Value::Integer(right)) => left.cmp(right),
        (Value::Fixed(left), Value::Fixed(right)) => left.cmp(right),
        _ => return Truth::Unknown,
    };

    Truth::from(match operator {
        Operator::Gt => ordering.is_gt(),
        Operator::Gte => ordering.is_ge(),
        Operator::Lt => ordering.is_lt(),
        Operator::Lte => ordering.is_le(),
        _ => return Truth::Unknown,
    })
}

fn status(state: &CanonicalState, subject: &Id, operator: Operator) -> Truth {
    let found = lookup(state, subject);

    match operator {
        Operator::Exists => Truth::from(found.is_some()),
        Operator::Known => Truth::from(matches!(
            found,
            Some((_, EpistemicStatus::Known | EpistemicStatus::Inferred))
        )),
        Operator::Unknown => Truth::from(match found {
            None => true,
            Some((_, status)) => matches!(status, EpistemicStatus::Unknown),
        }),
        _ => Truth::Unknown,
    }
}

impl From<bool> for Truth {
    fn from(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}

/// Outcome of a forward-chaining run.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// State after all inferences were applied.
    pub state: CanonicalState,
    /// Identifiers of rules that fired, in firing order.
    pub fired: Vec<Id>,
    /// Rounds executed before reaching the fixpoint.
    pub rounds: u32,
}

/// Errors from forward chaining.
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// A reach anchor matched neither an instance nor a sole-instance type.
    ///
    /// An error rather than an empty target set, and the distinction is
    /// load-bearing: `every` over an empty set is true, so a silently
    /// unresolved anchor would make every universal traversal hold and every
    /// rule containing one fire, with a well-formed proof saying so.
    #[error("reach anchor `{anchor}` matches no entity and no sole-instance type")]
    UnresolvedReachAnchor {
        /// The anchor that resolved to nothing.
        anchor: Id,
    },

    /// `collapse: the` was used where the traversal reached several targets.
    ///
    /// `the` asserts the traversal is functional. When it is not, the pack
    /// has said something false about its own topology, and choosing one
    /// target would be exactly the silent decision this mechanism exists to
    /// prevent. Declare `targets_per_source: one` on the relationship if the
    /// claim is meant to hold, and the state will enforce it.
    #[error("`the` reach from `{anchor}` reading `{read}` found {found} targets, not one")]
    AmbiguousReach {
        /// Where the traversal started.
        anchor: Id,
        /// The property it was reading.
        read: Id,
        /// How many it actually reached.
        found: usize,
    },

    /// The fixpoint loop exceeded [`MAX_ROUNDS`].
    #[error("forward chaining did not converge within {MAX_ROUNDS} rounds")]
    NoFixpoint,

    /// Applying an inference to state failed.
    #[error("applying inference failed: {0}")]
    Apply(#[from] dre_state::StateError),

    /// A rule referenced two different entity types.
    ///
    /// Binding a rule to one instance of one type is well defined. Binding
    /// it across two types is a join, and picking a semantics silently --
    /// cross product? pairwise? -- would make the answer depend on a choice
    /// nobody wrote down. Refused until a domain needs it.
    #[error(
        "rule {rule} references entity types {first} and {second}; a rule may bind to one type"
    )]
    MultipleTypes {
        /// The rule.
        rule: Id,
        /// First type referenced.
        first: Id,
        /// Second type referenced.
        second: Id,
    },

    /// A rule referenced a subject that is not `entity.property`.
    #[error("subject {subject} is not of the form entity.property")]
    MalformedSubject {
        /// The offending subject.
        subject: Id,
    },
}

/// Rewrite `type.property` to `instance.property`, leaving anything else.
pub(crate) fn rebind_subject(subject: &Id, type_id: &Id, instance: &Id) -> Id {
    match split_subject(subject) {
        Some((entity, property)) if entity == type_id.as_str() => {
            Id::new(format!("{instance}.{property}")).unwrap_or_else(|_| subject.clone())
        }
        _ => subject.clone(),
    }
}

/// Rewrite every subject in a condition tree for one binding.
pub(crate) fn rebind_condition(condition: &Condition, type_id: &Id, instance: &Id) -> Condition {
    match condition {
        Condition::Compare {
            subject,
            operator,
            operand,
        } => Condition::Compare {
            subject: rebind_subject(subject, type_id, instance),
            operator: *operator,
            operand: operand.clone(),
        },
        Condition::Reach {
            reach,
            collapse,
            operator,
            operand,
        } => Condition::Reach {
            reach: Reach {
                // The anchor is a BARE entity name, not a dotted subject, so
                // `rebind_subject` cannot do it — that splits at the last dot
                // and leaves an undotted name alone. Getting this wrong is
                // the design's own worst failure: an unrebound anchor
                // resolves to nothing, the target set is empty, and
                // `every` over an empty set is TRUE.
                from: if &reach.from == type_id {
                    instance.clone()
                } else {
                    reach.from.clone()
                },
                path: reach.path.clone(),
                read: reach.read.clone(),
                // Not rebound: these read properties on the entities the
                // traversal *reaches*, whose ids are computed at evaluation
                // time and never mention the anchor's type.
                also: reach.also.clone(),
            },
            collapse: *collapse,
            operator: *operator,
            operand: operand.clone(),
        },
        Condition::Status { subject, operator } => Condition::Status {
            subject: rebind_subject(subject, type_id, instance),
            operator: *operator,
        },
        Condition::All { all } => Condition::All {
            all: all
                .iter()
                .map(|inner| rebind_condition(inner, type_id, instance))
                .collect(),
        },
        Condition::Any { any } => Condition::Any {
            any: any
                .iter()
                .map(|inner| rebind_condition(inner, type_id, instance))
                .collect(),
        },
        Condition::Not { not } => Condition::Not {
            not: Box::new(rebind_condition(not, type_id, instance)),
        },
    }
}

/// A copy of the rule with every reference to `type_id` bound to `instance`.
fn rebind_rule(rule: &Rule, type_id: &Id, instance: &Id) -> Rule {
    let mut bound = rule.clone();
    bound.conditions = rule
        .conditions
        .iter()
        .map(|condition| rebind_condition(condition, type_id, instance))
        .collect();
    for inference in &mut bound.inferences {
        inference.subject = rebind_subject(&inference.subject, type_id, instance);
    }
    bound
}

/// Which instances a rule should be evaluated against.
///
/// A subject's first half names an entity *type* when the state contains
/// entities of that type; otherwise it names one entity directly, which is
/// how every rule worked before 2026-08-16 and still does.
///
/// Returning `None` means "evaluate the rule as written, once".
///
/// The type reading wins where a name is both, which is what keeps this
/// backwards compatible: every pack written in-house calls its single
/// entity the same thing as its type -- `pump` of type `pump` -- so binding
/// per instance binds to exactly one and nothing moves. That coincidence is
/// also why nobody noticed the limitation for the whole MVP. It took an
/// outside author modelling a switch, where the entity is `switch-1` and
/// the type is `ethernet_switch`, to find that a pack could describe one
/// named chassis and not a fleet.
fn bindings_for(rule: &Rule, state: &CanonicalState) -> Result<Vec<Option<Id>>, EvalError> {
    let mut subjects = referenced_subjects(&rule.conditions);
    subjects.extend(rule.inferences.iter().map(|i| i.subject.clone()));

    let mut named: Option<Id> = None;
    for subject in &subjects {
        let Some((entity_name, _)) = split_subject(subject) else {
            continue;
        };
        // An exact instance name wins over a type of the same name.
        //
        // Without this, a pack that calls its single server `server` and its
        // single endpoint `endpoint` -- which is what an author writes when
        // there is one of each -- has every cross-entity rule read as a join
        // between two types and refused. That is not a join. It is the
        // ordinary case of one rule reading one entity and concluding about
        // another, and it is the whole reason a pack declares more than one
        // entity.
        //
        // Found by the fourth independently authored pack, which modelled
        // OPC UA with six entity types and one instance of each. Every rule
        // spanning two of them was rejected, and the pack could not run at
        // all. Before 0.9.0 it would have worked, so the type-binding
        // feature had made a common shape unmodellable.
        //
        // `fleet`, which is what 0.9.0 was built for, is unaffected: its
        // instances are `sw-a`, `sw-b`, `sw-c` of type `switch`, and nothing
        // is called `switch`. A name is a type exactly when no instance
        // answers to it.
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
                return Err(EvalError::MultipleTypes {
                    rule: rule.id.clone(),
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

/// The declared type of one entity, or the entity's own name if unknown.
pub(crate) fn type_of(state: &CanonicalState, instance: &Id) -> Id {
    state
        .entities
        .get(instance)
        .map_or_else(|| instance.clone(), |entity| entity.type_id.clone())
}

/// Run rules to a fixpoint, recording each evaluation in the proof.
///
/// Rules are sorted once by [`Rule::firing_order`] and that order is
/// reused every round, so the sequence of activations is a function of the
/// rule set rather than of how the fixpoint happened to converge.
///
/// # Provenance
///
/// Each rule evaluation is linked to the nodes that established the
/// properties its conditions read, and each inference is linked to the
/// evaluation that produced it. That is what lets a conclusion be traced
/// past the rule to the measurement, and past the measurement to whichever
/// sources survived resolution — one edge at a time, with no step allowed
/// to claim a dependency it did not have.
pub fn forward_chain(
    rules: &[Rule],
    initial: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
    precedence: &dre_core::collections::DetMap<Id, Vec<Id>>,
) -> Result<Outcome, EvalError> {
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    ordered.sort_by(|left, right| left.firing_order(right));

    let mut state = initial;
    let mut fired: Vec<Id> = Vec::new();

    // Keyed on rule *and* binding. A rule naming an entity type is
    // evaluated once per instance, and each of those is a separate
    // activation with its own conclusions -- keying on the rule alone
    // would let the first switch that matched suppress every other one.
    let mut already_fired: std::collections::BTreeSet<(Id, Option<Id>)> =
        std::collections::BTreeSet::new();

    for round in 1..=MAX_ROUNDS {
        let mut changed = false;

        for rule in &ordered {
            for binding in bindings_for(rule, &state)? {
                // A rule that has already contributed its inferences cannot
                // contribute different ones later: its conclusions are fixed,
                // and the merge rule makes re-applying them a no-op. Skipping
                // keeps the proof free of duplicate activations.
                if already_fired.contains(&(rule.id.clone(), binding.clone())) {
                    continue;
                }

                let bound;
                let rule: &Rule = match &binding {
                    Some(instance) => {
                        bound = rebind_rule(rule, &type_of(&state, instance), instance);
                        &bound
                    }
                    None => rule,
                };

                context.charge_rule_evaluated();

                // Traversals are recorded before the evaluation that rests
                // on them, so the rule node can point at where it went. A
                // conclusion drawn across an edge has to be answerable for
                // which entities it reached; without this the proof says a
                // rule fired and gives no account of the graph it walked.
                let mut traversals = Vec::new();
                collect_reaches(&rule.conditions, &mut traversals);
                let mut reach_nodes = Vec::new();
                for reach_condition in &traversals {
                    let (reach, collapse) = reach_condition;
                    let targets = reach_targets(reach, &state)?;
                    reach_nodes.push(
                        trace.record(ProofNode {
                            kind: NodeKind::Reach {
                                anchor: reach.from.clone(),
                                path: reach
                                    .path
                                    .iter()
                                    .map(|hop| dre_proof::ReachHop {
                                        edge: hop.edge.clone(),
                                        outward: hop.direction == Direction::Out,
                                    })
                                    .collect(),
                                read: reach.read.clone(),
                                collapse: match collapse {
                                    Collapse::The => dre_proof::ReachCollapse::The,
                                    Collapse::Every => dre_proof::ReachCollapse::Every,
                                    Collapse::Some => dre_proof::ReachCollapse::Some,
                                    Collapse::AtLeast(count) => {
                                        dre_proof::ReachCollapse::AtLeast(*count)
                                    }
                                },
                                targets: targets.clone(),
                            },
                            // What a traversal rests on is the property it read
                            // on each entity it reached.
                            inputs: trace.sources_of(
                                targets
                                    .iter()
                                    .filter_map(|target| {
                                        Id::new(format!("{target}.{}", reach.read)).ok()
                                    })
                                    .collect::<Vec<_>>()
                                    .iter(),
                            ),
                        }),
                    );
                    context.charge_proof_node();
                }

                let mut holds = Truth::True;
                for condition in &rule.conditions {
                    holds = holds.and(evaluate(condition, &state)?);
                }

                // Recorded, not established: evaluating a rule reads state, it
                // does not set any. Treating it as an establishment would make
                // a later rule trace to the evaluation instead of to the value.
                let evaluation = trace.record(ProofNode {
                    kind: NodeKind::RuleEvaluated {
                        rule: rule.id.clone(),
                        fired: holds.is_true(),
                    },
                    inputs: trace.sources_of(referenced_subjects(&rule.conditions).iter()),
                });
                context.charge_proof_node();

                if !holds.is_true() {
                    continue;
                }

                state = apply_inferences(
                    rule,
                    state,
                    context.config().limits,
                    precedence,
                    trace,
                    context,
                )?;
                context.charge_rule_fired();
                context.charge_reasoning_step();
                already_fired.insert((rule.id.clone(), binding.clone()));
                fired.push(rule.id.clone());
                changed = true;

                for inference in &rule.inferences {
                    // Established: this node is now what the property rests on,
                    // so a rule that reads it later depends on this rule.
                    trace.establish(
                        inference.subject.clone(),
                        ProofNode {
                            kind: NodeKind::Inference {
                                subject: inference.subject.clone(),
                            },
                            inputs: vec![evaluation],
                        },
                    );
                    context.charge_proof_node();
                }
            }
        }

        if !changed {
            return Ok(Outcome {
                state,
                fired,
                rounds: round,
            });
        }
    }

    Err(EvalError::NoFixpoint)
}

/// Rebuild state with a rule's inferences merged in.
fn apply_inferences(
    rule: &Rule,
    state: CanonicalState,
    limits: dre_core::config::Limits,
    precedence: &dre_core::collections::DetMap<Id, Vec<Id>>,
    trace: &mut Trace,
    context: &mut ExecutionContext,
) -> Result<CanonicalState, EvalError> {
    let mut builder = StateBuilder::from_state(state, limits).with_precedence(precedence.clone());

    for inference in &rule.inferences {
        let (entity_name, property_name) =
            split_subject(&inference.subject).ok_or_else(|| EvalError::MalformedSubject {
                subject: inference.subject.clone(),
            })?;

        let entity = Id::new(entity_name).map_err(|_| EvalError::MalformedSubject {
            subject: inference.subject.clone(),
        })?;
        let property = Id::new(property_name).map_err(|_| EvalError::MalformedSubject {
            subject: inference.subject.clone(),
        })?;

        builder.infer(&entity, property, inference.value.clone())?;
    }

    // A value that beat another one says so. Without this the losing
    // conclusion would simply not be in the state, which reads exactly like
    // a rule that never fired.
    for (subject, winner, loser) in builder.take_overruled() {
        trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::TieBreak {
                winner: subject.clone(),
                rule: format!(
                    "declared precedence: {} dominates {}",
                    dre_state::render_value(&winner),
                    dre_state::render_value(&loser)
                ),
            },
            inputs: trace.sources_of(std::iter::once(&subject)),
        });
        context.charge_proof_node();
    }

    Ok(builder.build())
}

#[cfg(test)]
mod instance_type_binding_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use dre_core::config::Limits;
    use dre_state::{StateBuilder, Value};

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// A rule reading one entity and concluding about another, where both
    /// entities are named after their types.
    ///
    /// Not a join, and it must not be refused as one. It is the ordinary
    /// reason a pack declares more than one entity, and 0.9.0's type binding
    /// made it unmodellable: an OPC UA pack with six types and one instance
    /// of each had every cross-entity rule rejected and could not run.
    #[test]
    fn a_cross_entity_rule_is_not_a_join() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("server"), id("server")).expect("entity");
        builder
            .entity(id("endpoint"), id("endpoint"))
            .expect("entity");
        builder
            .observe(&id("server"), id("hardened"), Value::Boolean(false))
            .expect("observe");
        builder
            .declare(
                &id("endpoint"),
                id("acceptable"),
                dre_state::EpistemicStatus::Unknown,
            )
            .expect("declare");
        let state = builder.build();

        let rule = Rule {
            id: id("R001"),
            description: String::new(),
            conditions: vec![Condition::Compare {
                subject: id("server.hardened"),
                operator: crate::Operator::Eq,
                operand: Value::Boolean(false),
            }],
            inferences: vec![crate::Inference {
                subject: id("endpoint.acceptable"),
                value: Value::Boolean(false),
            }],
            priority: 100,
        };

        // No type binding at all: both names answer to an instance.
        assert_eq!(bindings_for(&rule, &state).expect("not a join"), vec![None]);
    }

    /// A genuine type reference still binds per instance, so the 0.9.0
    /// capability is intact. `fleet` depends on it, and its instances are
    /// `sw-a`/`sw-b`/`sw-c` with nothing called `switch`.
    #[test]
    fn a_name_no_instance_answers_to_is_still_a_type() {
        let mut builder = StateBuilder::new(Limits::default());
        for name in ["sw-a", "sw-b"] {
            builder.entity(id(name), id("switch")).expect("entity");
            builder
                .observe(&id(name), id("up"), Value::Boolean(true))
                .expect("observe");
        }
        let state = builder.build();

        let rule = Rule {
            id: id("R001"),
            description: String::new(),
            conditions: vec![Condition::Compare {
                subject: id("switch.up"),
                operator: crate::Operator::Eq,
                operand: Value::Boolean(true),
            }],
            inferences: Vec::new(),
            priority: 100,
        };

        assert_eq!(
            bindings_for(&rule, &state).expect("binds"),
            vec![Some(id("sw-a")), Some(id("sw-b"))]
        );
    }
}

#[cfg(test)]
mod reach_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::{Collapse, Hop};
    use dre_core::config::Limits;
    use dre_state::StateBuilder;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// A unit mounting two filesystems, one of which is unavailable.
    fn state() -> CanonicalState {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("web"), id("unit")).expect("entity");
        builder.entity(id("srv-data"), id("mount")).expect("entity");
        builder.entity(id("srv-logs"), id("mount")).expect("entity");
        builder
            .observe(&id("srv-data"), id("available"), Value::Boolean(true))
            .expect("observe");
        builder
            .observe(&id("srv-logs"), id("available"), Value::Boolean(false))
            .expect("observe");
        builder
            .relate(id("M1"), id("mounts"), id("web"), id("srv-data"))
            .expect("edge");
        builder
            .relate(id("M2"), id("mounts"), id("web"), id("srv-logs"))
            .expect("edge");
        builder.build()
    }

    fn reach(collapse: Collapse, operand: bool) -> Condition {
        Condition::Reach {
            reach: Reach {
                from: id("web"),
                path: vec![Hop {
                    edge: id("mounts"),
                    direction: Direction::Out,
                }],
                read: id("available"),
                also: Vec::new(),
            },
            collapse,
            operator: Operator::Eq,
            operand: Some(Value::Boolean(operand)),
        }
    }

    /// The capability four independent authors asked for: read a property on
    /// an entity you are related to, without binding a second type.
    #[test]
    fn a_traversal_reads_across_a_declared_edge() {
        let state = state();
        // One of two mounts is unavailable.
        assert_eq!(
            evaluate(&reach(Collapse::Some, false), &state).expect("evaluates"),
            Truth::True
        );
        // But not both.
        assert_eq!(
            evaluate(&reach(Collapse::Every, false), &state).expect("evaluates"),
            Truth::False
        );
    }

    /// `the` over a genuinely functional traversal reads the one target.
    ///
    /// Only the *refusal* was covered, which proves `the` can say no and not
    /// that it can say anything else. A collapse that refused every input
    /// would have passed the suite — and no shipped pack uses `the`, so
    /// nothing else would have noticed.
    #[test]
    fn the_over_one_target_reads_it() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("web"), id("unit")).expect("entity");
        builder.entity(id("srv-logs"), id("mount")).expect("entity");
        builder
            .observe(&id("srv-logs"), id("available"), Value::Boolean(false))
            .expect("observe");
        builder
            .relate(id("M1"), id("mounts"), id("web"), id("srv-logs"))
            .expect("edge");
        let sole = builder.build();

        assert_eq!(
            evaluate(&reach(Collapse::The, false), &sole).expect("evaluates"),
            Truth::True,
            "the one mount is unavailable, so `the ... == false` holds"
        );
        assert_eq!(
            evaluate(&reach(Collapse::The, true), &sole).expect("evaluates"),
            Truth::False,
            "and the opposite reading is definitely false, not unknown"
        );
    }

    /// `the` asserts the traversal is functional. When it is not, the pack
    /// has said something false about its own topology, and picking one
    /// target would be the silent decision this design exists to prevent.
    #[test]
    fn the_over_several_targets_is_refused_not_picked() {
        assert!(matches!(
            evaluate(&reach(Collapse::The, true), &state()),
            Err(EvalError::AmbiguousReach { found: 2, .. })
        ));
    }

    /// The design's own worst failure mode, guarded.
    ///
    /// `every` over an empty set is TRUE. So an anchor that silently
    /// resolved to nothing would make every universal traversal hold, every
    /// rule containing one fire, and the proof say so in well-formed prose.
    /// "Resolved and has no edges" and "did not resolve" must not share a
    /// representation.
    #[test]
    fn an_unresolved_anchor_is_an_error_not_an_empty_set() {
        let condition = Condition::Reach {
            reach: Reach {
                from: id("nonexistent"),
                path: vec![Hop {
                    edge: id("mounts"),
                    direction: Direction::Out,
                }],
                read: id("available"),
                also: Vec::new(),
            },
            collapse: Collapse::Every,
            operator: Operator::Eq,
            operand: Some(Value::Boolean(false)),
        };
        assert!(matches!(
            evaluate(&condition, &state()),
            Err(EvalError::UnresolvedReachAnchor { .. })
        ));
    }

    /// An anchor that resolves but has no edges of that type is a real
    /// answer, and vacuously true — which is only safe because the case
    /// above is an error.
    #[test]
    fn an_anchor_with_no_edges_is_vacuously_true() {
        let condition = Condition::Reach {
            reach: Reach {
                from: id("srv-data"),
                path: vec![Hop {
                    edge: id("mounts"),
                    direction: Direction::Out,
                }],
                read: id("available"),
                also: Vec::new(),
            },
            collapse: Collapse::Every,
            operator: Operator::Eq,
            operand: Some(Value::Boolean(false)),
        };
        assert_eq!(
            evaluate(&condition, &state()).expect("evaluates"),
            Truth::True
        );
    }
}

#[cfg(test)]
mod specificity_semantics_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::Inference;
    use dre_core::config::Limits;
    use dre_core::context::ExecutionContext;
    use dre_proof::Trace;
    use dre_state::StateBuilder;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// Specificity orders *firing*. It does not make a specific rule
    /// override a general one.
    ///
    /// The doc comment on `Rule::specificity` claimed it "makes specific
    /// rules override general defaults". It does not: both rules fire, and
    /// two inferences of different values for one property merge to
    /// CONFLICTING like any other disagreement. Overriding is what 0.14.0's
    /// declared value precedence is for.
    ///
    /// `model-packs/process` looked like a demonstration of overriding and
    /// is not — R003 requires `degraded UNKNOWN` and R004 requires
    /// `degraded EQ true`, so the two are mutually exclusive and only one
    /// ever fires. Nothing in the repository exercised the claim, which is
    /// why a false comment survived.
    #[test]
    fn specificity_orders_firing_and_does_not_override() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("pump"), id("pump")).expect("entity");
        builder
            .observe(&id("pump"), id("overloaded"), Value::Boolean(true))
            .expect("observe");
        builder
            .observe(&id("pump"), id("degraded"), Value::Boolean(true))
            .expect("observe");
        let state = builder.build();

        let general = Rule {
            id: id("R-general"),
            description: String::new(),
            conditions: vec![Condition::Compare {
                subject: id("pump.overloaded"),
                operator: Operator::Eq,
                operand: Value::Boolean(true),
            }],
            inferences: vec![Inference {
                subject: id("pump.advice"),
                value: Value::Symbol(id("reduce-load")),
            }],
            priority: 200,
        };
        let mut specific = general.clone();
        specific.id = id("R-specific");
        specific.conditions.push(Condition::Compare {
            subject: id("pump.degraded"),
            operator: Operator::Eq,
            operand: Value::Boolean(true),
        });
        specific.inferences = vec![Inference {
            subject: id("pump.advice"),
            value: Value::Symbol(id("service-now")),
        }];

        assert!(specific.specificity() > general.specificity());

        let mut context = ExecutionContext::with_defaults().expect("context");
        let mut trace = Trace::new();
        let outcome = forward_chain(
            &[general, specific],
            state,
            &mut context,
            &mut trace,
            &Default::default(),
        )
        .expect("converges");

        let advice = outcome
            .state
            .entities
            .get(&id("pump"))
            .expect("pump")
            .properties
            .get(&id("advice"))
            .expect("advice was written");

        // Both fire; neither overrides. The disagreement conflicts.
        assert_eq!(outcome.fired.len(), 2);
        assert_eq!(advice.status, dre_state::EpistemicStatus::Conflicting);
        assert!(advice.value.is_none());
    }
}

#[cfg(test)]
mod recursion_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::{Collapse, Hop, Inference};
    use dre_core::config::Limits;
    use dre_core::context::ExecutionContext;
    use dre_proof::Trace;
    use dre_state::StateBuilder;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// Declared reach plus the forward-chaining fixpoint already computes
    /// **transitive closure**, with one rule.
    ///
    /// Claimed by an independent audit and verified here rather than taken
    /// on trust. The mechanism: a rule anchored on a type reaches one hop,
    /// reads a property on what it reaches, and infers that same property on
    /// its anchor. Inferences apply immediately, and `already_fired` is keyed
    /// on `(rule, binding)` so a binding that has not yet fired is
    /// re-evaluated every round — so one rule propagates to a least fixed
    /// point without any closure operator in the path expression.
    ///
    /// This matters because `model-packs/fault-network` is 450 rules
    /// generated by a script to do exactly this propagation, one rule per
    /// edge. The capability that replaces them shipped in 0.18.0 and no pack
    /// uses it, which is this repository's most-repeated defect shape: built,
    /// hashed, and unexercised.
    #[test]
    fn one_reach_rule_propagates_transitively() {
        let mut builder = StateBuilder::new(Limits::default());
        // A chain: a -> b -> c -> d, and only d is impaired to begin with.
        for name in ["a", "b", "c", "d"] {
            builder.entity(id(name), id("node")).expect("entity");
        }
        builder
            .observe(&id("d"), id("impaired"), Value::Boolean(true))
            .expect("observe");
        for (edge, from, to) in [("E1", "a", "b"), ("E2", "b", "c"), ("E3", "c", "d")] {
            builder
                .relate(id(edge), id("depends_on"), id(from), id(to))
                .expect("edge");
        }
        let state = builder.build();

        let rule = Rule {
            id: id("R001"),
            description: String::new(),
            conditions: vec![Condition::Reach {
                reach: Reach {
                    from: id("node"),
                    path: vec![Hop {
                        edge: id("depends_on"),
                        direction: Direction::Out,
                    }],
                    read: id("impaired"),
                    also: Vec::new(),
                },
                collapse: Collapse::Some,
                operator: Operator::Eq,
                operand: Some(Value::Boolean(true)),
            }],
            inferences: vec![Inference {
                subject: id("node.impaired"),
                value: Value::Boolean(true),
            }],
            priority: 100,
        };

        let mut context = ExecutionContext::with_defaults().expect("context");
        let mut trace = Trace::new();
        let outcome = forward_chain(
            std::slice::from_ref(&rule),
            state,
            &mut context,
            &mut trace,
            &Default::default(),
        )
        .expect("converges");

        // Impairment reached every node upstream of `d`, three hops away,
        // from a single declared rule.
        for name in ["a", "b", "c"] {
            let property = outcome
                .state
                .entities
                .get(&id(name))
                .expect("entity")
                .properties
                .get(&id("impaired"))
                .unwrap_or_else(|| panic!("{name} should have been reached"));
            assert_eq!(
                property.value,
                Some(Value::Boolean(true)),
                "{name} is upstream of an impaired node"
            );
        }
    }
}

#[cfg(test)]
mod two_hop_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::{Collapse, Hop};
    use dre_core::config::Limits;
    use dre_state::StateBuilder;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    /// Two hops, out then back in — `service -> network -> service`.
    ///
    /// The shape the ninth independent author called "natural service
    /// reachability" and reported as missing. It is not missing: `path` is a
    /// list, `reach_targets` rebuilds a canonical frontier at each hop, and
    /// `check_reaches` validates that consecutive hops connect. What was
    /// missing is that **no pack has ever declared a two-edge path**, so the
    /// capability was untested and the brief showed only one hop.
    ///
    /// Verified rather than asserted, because "already built" is exactly the
    /// claim this repository has been wrong about before.
    #[test]
    fn a_two_hop_path_reaches_peers_on_a_shared_network() {
        let mut builder = StateBuilder::new(Limits::default());
        for name in ["console", "postgres", "grafana"] {
            builder.entity(id(name), id("service")).expect("entity");
        }
        for name in ["app", "data"] {
            builder.entity(id(name), id("network")).expect("entity");
        }
        // console and grafana share `app`; postgres is alone on `data`.
        for (edge, from, to) in [
            ("A1", "console", "app"),
            ("A2", "grafana", "app"),
            ("A3", "postgres", "data"),
        ] {
            builder
                .relate(id(edge), id("attached_to"), id(from), id(to))
                .expect("edge");
        }
        builder
            .observe(&id("grafana"), id("degraded"), Value::Boolean(true))
            .expect("observe");
        builder
            .observe(&id("console"), id("degraded"), Value::Boolean(false))
            .expect("observe");
        builder
            .observe(&id("postgres"), id("degraded"), Value::Boolean(false))
            .expect("observe");
        let state = builder.build();

        // From console: out along attached_to to `app`, back in to every
        // service on it. That reaches console itself and grafana.
        let peers = Reach {
            from: id("console"),
            path: vec![
                Hop {
                    edge: id("attached_to"),
                    direction: Direction::Out,
                },
                Hop {
                    edge: id("attached_to"),
                    direction: Direction::In,
                },
            ],
            read: id("degraded"),
            also: Vec::new(),
        };

        let condition = Condition::Reach {
            reach: peers.clone(),
            collapse: Collapse::Some,
            operator: Operator::Eq,
            operand: Some(Value::Boolean(true)),
        };
        assert_eq!(
            evaluate(&condition, &state).expect("evaluates"),
            Truth::True,
            "grafana is a degraded peer of console on the app network"
        );

        // postgres is on `data` alone, so it is not reached and cannot be
        // what made this true.
        let targets = reach_targets(&peers, &state).expect("resolves");
        assert!(targets.contains(&id("grafana")));
        assert!(!targets.contains(&id("postgres")));
    }
}

#[cfg(test)]
mod at_least_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::{Collapse, Hop};
    use dre_core::config::Limits;
    use dre_state::{EpistemicStatus, StateBuilder};

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn at_least(n: u32, state: &CanonicalState) -> Truth {
        evaluate(
            &Condition::Reach {
                reach: Reach {
                    from: id("svc"),
                    path: vec![Hop {
                        edge: id("attached_to"),
                        direction: Direction::Out,
                    }],
                    read: id("healthy"),
                    also: Vec::new(),
                },
                collapse: Collapse::AtLeast(n),
                operator: Operator::Eq,
                operand: Some(Value::Boolean(true)),
            },
            state,
        )
        .expect("evaluates")
    }

    /// Two healthy networks and one undecided.
    fn state() -> CanonicalState {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("svc"), id("service")).expect("entity");
        for (name, healthy) in [("n1", Some(true)), ("n2", Some(true)), ("n3", None)] {
            builder.entity(id(name), id("network")).expect("entity");
            match healthy {
                Some(value) => builder
                    .observe(&id(name), id("healthy"), Value::Boolean(value))
                    .map(|_| ()),
                None => builder
                    .declare(&id(name), id("healthy"), EpistemicStatus::Unknown)
                    .map(|_| ()),
            }
            .expect("property");
            builder
                .relate(
                    Id::new(format!("E-{name}")).expect("id"),
                    id("attached_to"),
                    id("svc"),
                    id(name),
                )
                .expect("edge");
        }
        builder.build()
    }

    /// The one thing the ninth author asked for that was a genuine gap:
    /// "topology often cares about 'attached to at least two networks'".
    #[test]
    fn counting_is_three_valued_not_two() {
        let state = state();

        // Two are definitely healthy.
        assert_eq!(at_least(2, &state), Truth::True);

        // A third could still be, so three is not yet false.
        assert_eq!(at_least(3, &state), Truth::Unknown);

        // A fourth cannot be: only three targets exist.
        assert_eq!(at_least(4, &state), Truth::False);
    }
}
