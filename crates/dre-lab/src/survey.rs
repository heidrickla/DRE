//! Active evidence gathering — plan section 26.
//!
//! "Which single observation would most change what I decide?"
//!
//! The second post-MVP track, and like the first it adds no reasoning. A
//! survey is a counterfactual run once per candidate value of every unknown
//! the experiment declares, and a ranking over the results. Every individual
//! answer is an ordinary run.
//!
//! # Where the candidate values come from
//!
//! The ontology, which is the only place that says what a property may
//! hold. A boolean has two candidates; a symbol with a declared vocabulary
//! has one per symbol. That makes this the first thing in the repository to
//! read `ontology.yaml` for something other than validation — the file was
//! hashed and unread for the whole MVP, and closing a vocabulary is what
//! now makes a property surveyable at all.
//!
//! # Numerics, and the argument that was half right
//!
//! This used to decline every numeric: an unbounded property has no
//! enumerable candidate set, and sampling the bounds would find threshold
//! flips near the extremes and silently miss every one in the middle.
//!
//! The objection was sound and the conclusion did not follow. **The middle is
//! exactly where the thresholds are, and the pack has already written them
//! down.** A numeric's effect on whether a rule fires is a step function whose
//! breakpoints are the operands the pack declares, so probing each declared
//! threshold — and its two neighbours, because `GT` and `GTE` differ exactly
//! there — is *exact for the layer that matters* rather than a sample of it.
//!
//! A numeric the pack compares against nothing is still declined by name,
//! because it genuinely has no breakpoint to probe. So is one the pack reads
//! only to compute something else: a pack that subtracts a baseline and
//! thresholds the difference has put the breakpoint one step downstream, and
//! carrying a threshold back across arithmetic is interval reasoning this
//! engine does not do. That case says which derived value holds the
//! threshold, because "never compared against anything" reads as "unused" and
//! the property it was first said about was the one an expert chose to
//! measure next.
//!
//! # Two questions, and the second one needs the explanation layer
//!
//! Originally this asked only: **can knowing this change the decision, and
//! how**. That is answerable with no reasoning beyond a re-run, which is why
//! it came first.
//!
//! With hypotheses declared there is a sharper question available — *which
//! observation best separates the accounts still in contention* — and it is
//! a different question, not a refinement. An observation can change the
//! decision without discriminating at all, if every account moves together;
//! and one that leaves the plan untouched can still be the measurement that
//! settles which account is right. The report gives both, and ranks on the
//! second where a pack declares an explanation layer.
//!
//! This is where the engine starts behaving like an investigator rather than
//! a rule evaluator: it is deterministic experimental design, computed by
//! running the same ranking over each admissible answer and watching the
//! leader's standing move.
//!
//! # What "valuable" means here
//!
//! Deliberately narrow: **can knowing this change the decision, and how**.
//! The engine has no notion of a better world in general — only, where a
//! pack plans, a cheaper one. So a survey reports which values are reachable
//! and what each costs, and leaves "worth it" to the reader. A property
//! whose every value leaves the decision alone is not worth measuring, and
//! that is a claim this can make honestly.

use std::collections::BTreeSet;

use dre_core::id::Id;
use dre_model::{parse_section, ModelPack, SectionKind};
use dre_state::ontology::{OntologySection, PropertyKind};
use dre_state::Value;

use dre_core::fixed::Fixed;
use dre_state::CanonicalState;
use dre_symbolic::features::{
    evaluate as evaluate_expression, referenced_subjects as expression_subjects,
};
use dre_symbolic::{Condition, Derived, Expression, Operator, Truth};

use crate::counterfactual::{compare, CounterfactualError, Perturbation};
use crate::experiment::Experiment;

/// What one candidate value would lead to.
#[derive(Debug)]
pub struct Outcome {
    /// The value tried.
    pub value: Value,
    /// Whether the engine concluded anything different.
    ///
    /// Deliberately not the `DecisionHash`, which moves whenever the input
    /// does — see `Comparison::conclusion_changed`. Ranking on the hash
    /// made every unknown look decisive.
    pub decision_changed: bool,
    /// The plan's cost under this value, where the pack plans.
    pub plan_cost: Option<dre_core::fixed::Fixed>,
    /// How many steps that plan had.
    pub plan_steps: Option<usize>,
    /// Which account would lead, where the pack declares hypotheses.
    pub leader: Option<Id>,
    /// How far ahead the *base* leader would be, which may be negative.
    ///
    /// Held to the hypothesis that leads as the experiment is written, not
    /// to whoever leads under this value. Re-reading the current leader
    /// would change subject exactly when the answer got interesting, and a
    /// flip would look like no movement at all.
    pub lead: Option<dre_core::fixed::Fixed>,
}

/// One unknown property and what measuring it would buy.
#[derive(Debug)]
pub struct Surveyed {
    /// `entity.property`.
    pub subject: Id,
    /// One entry per candidate value the ontology admits.
    pub outcomes: Vec<Outcome>,
}

impl Surveyed {
    /// How far this observation's answers could move the leading account.
    ///
    /// The spread between the best and worst standing the base leader could
    /// end up with. This is the question recommendation 12 asks and the
    /// existing ranking cannot: not "could knowing this change the
    /// decision", but "which measurement best separates the accounts that
    /// are still in contention".
    ///
    /// Zero means the answers are interchangeable as far as the diagnosis
    /// goes. It does not mean the observation is worthless in general —
    /// `door_ajar` may matter enormously to somebody — only that no
    /// admissible answer moves one account relative to another, so it
    /// cannot settle which is right.
    #[must_use]
    pub fn discrimination(&self) -> Option<dre_core::fixed::Fixed> {
        let leads: Vec<dre_core::fixed::Fixed> = self
            .outcomes
            .iter()
            .filter_map(|outcome| outcome.lead)
            .collect();
        let best = leads.iter().copied().max()?;
        let worst = leads.iter().copied().min()?;
        best.checked_sub(worst).ok()
    }

    /// Whether some admissible answer would put a different account in
    /// front.
    ///
    /// Strictly stronger than a large [`Self::discrimination`], and worth
    /// separating: an observation that widens a lead confirms a diagnosis,
    /// and one that overturns it is the reason to go and measure something
    /// before acting.
    #[must_use]
    pub fn flips_leader(&self) -> bool {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.lead)
            .any(|lead| lead < dre_core::fixed::Fixed::ZERO)
    }

    /// Whether any admissible value changes the decision.
    #[must_use]
    pub fn decisive(&self) -> bool {
        self.outcomes.iter().any(|outcome| outcome.decision_changed)
    }

    /// How many distinct decisions this property can lead to, counting the
    /// base. A property reaching three decisions is more informative than
    /// one reaching two, and both beat one.
    #[must_use]
    pub fn distinct_decisions(&self) -> usize {
        let changed = self
            .outcomes
            .iter()
            .filter(|outcome| outcome.decision_changed)
            .count();
        if changed == 0 {
            1
        } else {
            changed + 1
        }
    }

    /// The cheapest plan any admissible value leads to.
    ///
    /// The only sense in which this engine can call one outcome better than
    /// another. It is not "the value of the measurement" — you do not get
    /// to choose what the world turns out to be — but the spread between
    /// this and [`Self::worst_cost`] is what an answer is worth finding out.
    #[must_use]
    pub fn best_cost(&self) -> Option<dre_core::fixed::Fixed> {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.plan_cost)
            .min()
    }

    /// The dearest plan any admissible value leads to.
    #[must_use]
    pub fn worst_cost(&self) -> Option<dre_core::fixed::Fixed> {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.plan_cost)
            .max()
    }
}

/// An unknown that could not be enumerated, and why.
#[derive(Debug)]
pub struct NotSurveyable {
    /// `entity.property`.
    pub subject: Id,
    /// Why it was declined.
    pub reason: String,
}

/// The result of surveying an experiment's unknowns.
#[derive(Debug)]
pub struct Survey {
    /// The decision the experiment reaches as written.
    pub base_decision: dre_core::hash::Hash32,
    /// The plan cost as written, where the pack plans.
    pub base_cost: Option<dre_core::fixed::Fixed>,
    /// The account leading as the experiment is written, if any.
    pub base_leader: Option<Id>,
    /// How far ahead it is.
    pub base_lead: Option<dre_core::fixed::Fixed>,
    /// Surveyed unknowns, most informative first.
    pub surveyed: Vec<Surveyed>,
    /// Unknowns that were declined, by name, so their absence is visible.
    pub declined: Vec<NotSurveyable>,
}

/// Every subject the pack computes for itself.
///
/// This is **not** an observability filter, and calling it one was a
/// mistake worth not repeating. Observability here means whether the engine
/// has a defined way to obtain or inspect a fact — not whether the fact
/// currently holds a value — and `EpistemicStatus` already says it:
/// `Unknown` is "not observed, but observable in principle" and
/// `Unobservable` is "no amount of evidence gathering will resolve it". The
/// `Unknown` filter in [`survey`] is therefore already the observability
/// question, answered by the model rather than guessed at here.
///
/// What the format does not say is *which route* a fact has to a value.
/// A derived property is perfectly observable in the sense above — the
/// engine obtains it by computing it — but the route is internal, so there
/// is nothing to go and acquire. That is the distinction this reconstructs:
/// not observable versus unobservable, but obtained-by-derivation versus
/// obtained-from-outside.
///
/// Reconstructed from what the pack already declares — feature subjects,
/// rule inference subjects, geometry outputs — so it needs no format
/// change. Measured on `fault-network`, it is the difference between a
/// usable report and 240 entries, because that pack declares every
/// conclusion UNKNOWN so the resolution metric has a denominator. Those
/// conclusions are observable and they are not acquirable, and only the
/// second fact makes them uninteresting to a survey.
fn derived_subjects(pack: &ModelPack, experiment: &Experiment) -> BTreeSet<Id> {
    let mut derived = BTreeSet::new();

    if let Ok(Some(section)) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)
    {
        derived.extend(section.features.into_iter().map(|feature| feature.subject));
    }

    if let Ok(Some(section)) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)
    {
        for rule in section.rules {
            derived.extend(
                rule.inferences
                    .into_iter()
                    .map(|inference| inference.subject),
            );
        }
    }

    // The property the ranking writes is derived too, and this was missed
    // the first time: `device-cascade-abduction-004` offered
    // `boiler-1.root_cause` as something to go and measure, alongside a
    // straight-faced report of what the engine would conclude if told its
    // own conclusion.
    //
    // The same shape as the `fault-network` finding that put this function
    // here, and the same shape as every other declared-but-unread defect in
    // this repository: a new section arrived carrying a subject, and the
    // code that enumerates derived subjects went on reading the sections it
    // already knew about. Worth noting that nothing failed — the survey ran,
    // sorted, and produced a confident entry that was nonsense.
    if let Ok(Some(section)) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)
    {
        derived.extend(section.rank_into);
    }

    // A space names an entity *type* and writes onto every instance of it,
    // so the derived subjects are per entity rather than per space.
    if let Ok(Some(section)) =
        parse_section::<dre_geometry::GeometrySection>(pack, SectionKind::Geometry)
    {
        let written: Vec<&Id> = [
            Some(&section.space.classify_into),
            section.space.boundary_distance_into.as_ref(),
            section.space.inside_region_into.as_ref(),
        ]
        .into_iter()
        .flatten()
        .collect();

        for entity in &experiment.given.entities {
            if entity.type_id != section.space.subject {
                continue;
            }
            for property in &written {
                if let Ok(subject) = Id::new(format!("{}.{}", entity.id, property)) {
                    derived.insert(subject);
                }
            }
        }
    }

    derived
}

/// Whether a subject written in a pack names this property.
///
/// A pack writes `incident.db_lock_wait_ms`, where `incident` is an entity
/// **type**; the survey builds `incident-001.db_lock_wait_ms`, where
/// `incident-001` is the **instance** the experiment declared. The engine
/// binds a type-form subject to each instance of that type, so both spellings
/// name the same measurement and the survey has to accept either.
///
/// This was invisible for as long as the only pack exercised here happened to
/// name its instance exactly after its type. A second pack, independently
/// authored, named its instance `incident-001` and had all three of its
/// measurements declined as properties "the pack never compares against
/// anything" — while the pack thresholds a value derived from each of them.
fn names_the_same(written: &Id, aliases: &[Id]) -> bool {
    aliases.iter().any(|alias| alias == written)
}

/// Both spellings of one property: as the experiment instantiated it, and as
/// a pack would write it against the entity type.
fn aliases_of(entity: &Id, type_id: &Id, property: &Id) -> Vec<Id> {
    let mut found = Vec::new();
    if let Ok(subject) = Id::new(format!("{entity}.{property}")) {
        found.push(subject);
    }
    if type_id != entity {
        if let Ok(subject) = Id::new(format!("{type_id}.{property}")) {
            found.push(subject);
        }
    }
    found
}

/// Every literal the pack compares a subject against, anywhere.
///
/// Rules, feature guards, constraints, goals and an explanation's
/// observations all fold `Condition`s, and every one of them can put a
/// threshold on a numeric property.
fn thresholds_for(pack: &ModelPack, aliases: &[Id]) -> Vec<(Operator, Value)> {
    fn walk(condition: &dre_symbolic::Condition, aliases: &[Id], out: &mut Vec<(Operator, Value)>) {
        match condition {
            dre_symbolic::Condition::Compare {
                subject: found,
                operand,
                operator,
            } => {
                if names_the_same(found, aliases) {
                    out.push((*operator, operand.clone()));
                }
            }
            dre_symbolic::Condition::All { all: nested }
            | dre_symbolic::Condition::Any { any: nested } => {
                for inner in nested {
                    walk(inner, aliases, out);
                }
            }
            dre_symbolic::Condition::Not { not } => walk(not, aliases, out),
            dre_symbolic::Condition::Status { .. } | dre_symbolic::Condition::Reach { .. } => {}
        }
    }

    let mut found = Vec::new();

    if let Ok(Some(section)) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)
    {
        for rule in &section.rules {
            for condition in &rule.conditions {
                walk(condition, aliases, &mut found);
            }
        }
    }
    if let Ok(Some(section)) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)
    {
        for feature in &section.features {
            for condition in &feature.when {
                walk(condition, aliases, &mut found);
            }
        }
    }
    if let Ok(Some(section)) =
        parse_section::<dre_planner::ConstraintsSection>(pack, SectionKind::Constraints)
    {
        for constraint in &section.constraints {
            for condition in &constraint.conditions {
                walk(condition, aliases, &mut found);
            }
        }
    }
    if let Ok(Some(section)) = parse_section::<dre_planner::GoalsSection>(pack, SectionKind::Goals)
    {
        for goal in &section.goals {
            for condition in &goal.conditions {
                walk(condition, aliases, &mut found);
            }
        }
    }
    if let Ok(Some(section)) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)
    {
        for observation in &section.observations {
            walk(&observation.when, aliases, &mut found);
        }
    }

    found
}

/// Derived properties whose computation reads `subject`.
///
/// A numeric is often not compared directly. The pack subtracts a baseline
/// from it and thresholds *that*, which is ordinary and good modelling — and
/// it means [`thresholds_for`] finds nothing, because the threshold lives one
/// step downstream on a different property.
///
/// Propagating a threshold backwards across arithmetic is a real capability
/// this engine does not have; inverting `subtract` is easy and inverting the
/// general case is interval reasoning over an expression tree. Until it
/// exists, the least this can do is say which derived property the threshold
/// actually sits on, rather than claim the pack never compares the property
/// against anything — which reads as "this property is unused" and is exactly
/// the wrong thing to tell someone deciding what to go and measure.
fn feeds_derived(pack: &ModelPack, aliases: &[Id]) -> Vec<Id> {
    fn reads(expression: &dre_symbolic::Expression, aliases: &[Id]) -> bool {
        use dre_symbolic::Expression as E;
        match expression {
            E::Subject(id) => names_the_same(id, aliases),
            E::Literal(_) => false,
            E::Add(parts)
            | E::Subtract(parts)
            | E::Multiply(parts)
            | E::Divide(parts)
            | E::Min(parts)
            | E::Max(parts) => parts.iter().any(|part| reads(part, aliases)),
            E::Abs(inner) | E::Negate(inner) => reads(inner, aliases),
        }
    }

    let mut found = Vec::new();
    if let Ok(Some(section)) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)
    {
        for feature in &section.features {
            if reads(&feature.expression, aliases) {
                found.push(feature.subject.clone());
            }
        }
    }
    found
}

/// Candidate values for a numeric property, taken from the thresholds the
/// pack itself compares it against.
///
/// **Not sampling. Exact, for the layer that matters.** A numeric property's
/// effect on whether a rule fires is a step function, and its breakpoints are
/// exactly the operands the pack declares. Between two adjacent thresholds
/// every condition in the pack behaves identically, so one probe per interval
/// is complete rather than approximate — which is why this answers the
/// standing objection to enumerating numerics ("sampling the bounds would
/// find threshold flips near the extremes and silently miss every one in the
/// middle"). The middle is where the thresholds are, and the pack has already
/// written them down.
///
/// The domain that forced it: an IT diagnosis where the ranking sat 0.20
/// apart and **all three** measurements that could settle it were numeric —
/// two integers in milliseconds and a fixed-point percentage. The survey
/// declined every one of them and reported "no single observation would
/// change this decision", while an outside author, working blind, named one
/// of the three and reasoned about it entirely in terms of the pack's own
/// thresholds. IT diagnosis is latency, lag and queue depth; a discrimination
/// metric that cannot consider numbers cannot do it.
///
/// Each threshold contributes three probes — just below, at, and just above —
/// because `GT` and `GTE` differ exactly there, and a metric that could not
/// tell them apart would be guessing about the boundary it was asked to
/// examine.
fn numeric_probes(pack: &ModelPack, aliases: &[Id], kind: PropertyKind) -> Vec<Value> {
    let step = match kind {
        PropertyKind::Integer => None,
        _ => Some(dre_core::fixed::Fixed::from_raw(1)),
    };

    let mut probes: Vec<Value> = Vec::new();
    for (_, threshold) in thresholds_for(pack, aliases) {
        match (&threshold, step) {
            (Value::Integer(at), None) => {
                for candidate in [at.saturating_sub(1), *at, at.saturating_add(1)] {
                    probes.push(Value::Integer(candidate));
                }
            }
            (Value::Fixed(at), Some(unit)) => {
                probes.push(Value::Fixed(at.checked_sub(unit).unwrap_or(*at)));
                probes.push(Value::Fixed(*at));
                probes.push(Value::Fixed(at.checked_add(unit).unwrap_or(*at)));
            }
            _ => {}
        }
    }

    ordered(&mut probes);
    probes
}

/// Deterministic and duplicate-free.
///
/// Thresholds one unit apart in different rules would otherwise probe the
/// same value twice and run the same counterfactual twice. The vector is
/// homogeneous in kind by construction — probes are emitted in the
/// property's declared kind on both paths — so the mixed-pair arm never
/// runs.
fn ordered(values: &mut Vec<Value>) {
    values.sort_by(|left, right| match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Fixed(a), Value::Fixed(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    });
    values.dedup();
}

/// What a numeric property should be probed at, and what the run must
/// confirm afterwards.
struct Probes {
    /// Everything to try, sorted and deduplicated.
    values: Vec<Value>,
    /// The subset a threshold on the property itself justified. Read only as
    /// the fallback when a carried threshold fails its check.
    direct: Vec<Value>,
    /// The declared kind, so a post-run refusal is worded like the others.
    kind: PropertyKind,
    /// One per carried threshold. Empty means nothing was carried.
    checks: Vec<Carried>,
}

impl Probes {
    /// A probe set with nothing carried and nothing to confirm.
    fn plain(values: Vec<Value>, kind: PropertyKind) -> Self {
        Self {
            values,
            direct: Vec::new(),
            kind,
            checks: Vec::new(),
        }
    }
}

/// A threshold carried back, held so the run can be asked whether the probes
/// really cross it.
struct Carried {
    /// The feature subject the threshold sits on.
    derived: Id,
    operator: Operator,
    /// Always fixed-point; a threshold spelled otherwise is declined.
    threshold: Value,
}

/// Everything carry-back produced for one property.
#[derive(Default)]
struct CarryBack {
    values: Vec<Value>,
    checks: Vec<Carried>,
    refusals: Vec<(Id, Why)>,
}

impl CarryBack {
    fn refuse(&mut self, derived: Id, why: Why) {
        self.refusals.push((derived, why));
    }
}

/// Why one feature could not carry its threshold back.
enum Why {
    /// `multiply`, `divide`, `min`, `max` or `abs` on the route down.
    Blocked(&'static str),
    ReadTwice(usize),
    SiblingUnreadable(Vec<Id>),
    SiblingUnusable(String),
    SlopeNotUnit,
    Overflow,
    OperandNotFixed(String),
    EqualityOffGrid(i64, i64),
    OutsideDomain(String),
    SeedDidNotCross,
    NothingComparesTheDerived,
}

/// The grid the surveyed property moves on.
#[derive(Clone, Copy)]
enum Step {
    /// An `integer` property moves in whole numbers.
    Whole,
    /// A `fixed` property moves in raw units of 1e-6.
    Raw,
}

/// The first operation on the route down to `subject` that cannot be undone.
///
/// Only the **path** is constrained. A sibling subtree free of `subject` may
/// be any shape at all, because it does not vary with `subject` and is folded
/// by the engine's own evaluator rather than reasoned about here. So
/// `subtract: [subject: X, multiply: [...]]` is in scope, and the multiply's
/// truncation is reproduced rather than inverted.
///
/// Exhaustive, with no `_` arm, so a new `Expression` variant is a compile
/// error instead of a silently wrong answer.
fn blocking_operation(expression: &Expression, subject: &Id) -> Option<&'static str> {
    fn holds(expression: &Expression, subject: &Id) -> bool {
        expression_subjects(expression)
            .iter()
            .any(|id| id == subject)
    }
    fn any_holds(parts: &[Expression], subject: &Id) -> bool {
        parts.iter().any(|part| holds(part, subject))
    }

    match expression {
        Expression::Subject(_) | Expression::Literal(_) => None,

        // Unit-preserving and exact on the raw i64. Descend into whichever
        // operand carries the subject; the others are constants.
        Expression::Add(parts) | Expression::Subtract(parts) => parts
            .iter()
            .find(|part| holds(part, subject))
            .and_then(|part| blocking_operation(part, subject)),
        Expression::Negate(inner) => blocking_operation(inner, subject),

        // Blocked only when the subject is below them. As siblings they are
        // ordinary constants and never reach here.
        Expression::Multiply(parts) => any_holds(parts, subject).then_some("multiply"),
        Expression::Divide(parts) => any_holds(parts, subject).then_some("divide"),
        Expression::Min(parts) => any_holds(parts, subject).then_some("min"),
        Expression::Max(parts) => any_holds(parts, subject).then_some("max"),
        Expression::Abs(inner) => holds(inner, subject).then_some("abs"),
    }
}

/// Replace every occurrence of `subject` with a literal reading.
///
/// Faithful because `features::read` converts an integer property to
/// `Fixed::from_int` and a fixed one to itself — the same conversion done
/// here. Substituting into the expression rather than into a state needs no
/// `StateBuilder`, no limits, and no decision about whether the reading is
/// observed or set, and it evaluates the siblings through the same function
/// the engine runs.
fn substitute(expression: &Expression, subject: &Id, reading: Fixed) -> Expression {
    let each = |parts: &Vec<Expression>| -> Vec<Expression> {
        parts
            .iter()
            .map(|part| substitute(part, subject, reading))
            .collect()
    };
    match expression {
        Expression::Subject(id) if id == subject => Expression::Literal(reading),
        Expression::Subject(id) => Expression::Subject(id.clone()),
        Expression::Literal(value) => Expression::Literal(*value),
        Expression::Add(parts) => Expression::Add(each(parts)),
        Expression::Subtract(parts) => Expression::Subtract(each(parts)),
        Expression::Multiply(parts) => Expression::Multiply(each(parts)),
        Expression::Divide(parts) => Expression::Divide(each(parts)),
        Expression::Min(parts) => Expression::Min(each(parts)),
        Expression::Max(parts) => Expression::Max(each(parts)),
        Expression::Abs(inner) => Expression::Abs(Box::new(substitute(inner, subject, reading))),
        Expression::Negate(inner) => {
            Expression::Negate(Box::new(substitute(inner, subject, reading)))
        }
    }
}

/// Which of the expression's other subjects the state cannot read.
fn unreadable(expression: &Expression, subject: &Id, state: &CanonicalState) -> Vec<Id> {
    let mut named: BTreeSet<Id> = BTreeSet::new();
    for id in expression_subjects(expression) {
        if &id == subject {
            continue;
        }
        if !matches!(
            evaluate_expression(&Expression::Subject(id.clone()), state),
            Derived::Value(_)
        ) {
            named.insert(id);
        }
    }
    named.into_iter().collect()
}

/// What the feature computes when `subject` reads `reading`.
fn value_at(
    expression: &Expression,
    subject: &Id,
    reading: Fixed,
    state: &CanonicalState,
) -> Result<Fixed, Why> {
    match evaluate_expression(&substitute(expression, subject, reading), state) {
        Derived::Value(value) => Ok(value),
        // `evaluate` short-circuits and does not say which operand was
        // missing, so ask each referenced subject on its own.
        Derived::Unknown => Err(Why::SiblingUnreadable(unreadable(
            expression, subject, state,
        ))),
        Derived::Invalid(why) => Err(Why::SiblingUnusable(why)),
        Derived::NotApplicable => Err(Why::SiblingUnusable("not applicable here".to_owned())),
    }
}

/// Measure `derived = ±reading + offset` by asking the engine, twice.
///
/// Sampled at whole zero and whole one, never at one raw unit: an
/// `integer`-declared property cannot take 0.000001, so a raw-unit step would
/// measure a slope of zero and refuse its own domain.
///
/// Nothing here re-implements the arithmetic in `features::evaluate`. It
/// calls it. A backward transfer function per operator would be a second
/// reader of the same declaration, which is the shape of nine previous
/// defects in this repository.
fn line(
    expression: &Expression,
    subject: &Id,
    state: &CanonicalState,
) -> Result<(bool, Fixed), Why> {
    let at_zero = value_at(expression, subject, Fixed::ZERO, state)?;
    let at_one = value_at(expression, subject, Fixed::ONE, state)?;
    let slope = at_one.checked_sub(at_zero).map_err(|_| Why::Overflow)?;

    // `blocking_operation` already said this holds. Checked anyway: it costs
    // one comparison and is a second, independent guard against the subject
    // occurring twice. A gate bug becomes a refusal, not a wrong probe.
    if slope.raw() == Fixed::SCALE {
        Ok((true, at_zero))
    } else if slope.raw() == -Fixed::SCALE {
        Ok((false, at_zero))
    } else {
        Err(Why::SlopeNotUnit)
    }
}

/// The reading that puts the derived value exactly on its threshold.
///
/// Exact: one `checked_sub` on the raw i64, no rescaling, no rounding.
fn boundary(rising: bool, offset: Fixed, threshold: Fixed) -> Result<Fixed, Why> {
    if rising {
        threshold.checked_sub(offset)
    } else {
        offset.checked_sub(threshold)
    }
    .map_err(|_| Why::Overflow)
}

/// The readings either side of a boundary, on the property's own grid.
///
/// Three when the boundary is a reading the property can take, because `GT`
/// and `GTE` differ exactly there. Two when it falls between two whole
/// numbers, because no integer sits on it.
fn grid(at: Fixed, step: Step) -> Result<Vec<Value>, Why> {
    match step {
        Step::Raw => {
            let unit = Fixed::from_raw(1);
            let below = at.checked_sub(unit).map_err(|_| Why::Overflow)?;
            let above = at.checked_add(unit).map_err(|_| Why::Overflow)?;
            Ok(vec![
                Value::Fixed(below),
                Value::Fixed(at),
                Value::Fixed(above),
            ])
        }
        Step::Whole => {
            let whole = at.raw().div_euclid(Fixed::SCALE);
            if at.raw().rem_euclid(Fixed::SCALE) == 0 {
                Ok(vec![
                    Value::Integer(whole.saturating_sub(1)),
                    Value::Integer(whole),
                    Value::Integer(whole.saturating_add(1)),
                ])
            } else {
                // The flip lies strictly between these two, so both are
                // needed and nothing sits on the boundary itself.
                Ok(vec![
                    Value::Integer(whole),
                    Value::Integer(whole.saturating_add(1)),
                ])
            }
        }
    }
}

/// A probe as fixed-point, for forward evaluation.
fn numeric(value: &Value) -> Option<Fixed> {
    match value {
        Value::Integer(at) => Fixed::from_int(*at).ok(),
        Value::Fixed(at) => Some(*at),
        _ => None,
    }
}

/// Which side of the threshold a computed value falls on.
fn side_of(value: Fixed, operator: Operator, threshold: Fixed) -> Option<bool> {
    match operator {
        Operator::Gt => Some(value > threshold),
        Operator::Gte => Some(value >= threshold),
        Operator::Lt => Some(value < threshold),
        Operator::Lte => Some(value <= threshold),
        Operator::Eq => Some(value == threshold),
        Operator::Ne => Some(value != threshold),
        _ => None,
    }
}

/// Probes obtained by carrying thresholds back across the features that read
/// this property.
///
/// The domain that forced it: an IT pack thresholds `write_delta_ms`, which
/// it computes by subtracting a baseline from `write_p95_ms`. The raw
/// measurement is therefore compared against nothing directly and cannot be
/// probed — and it was the measurement an outside expert chose to take next.
/// A second, independently authored pack does the same thing three more
/// times. IT measures almost everything against a normal, so this shape is
/// the common case rather than an edge one.
fn carry_back(
    pack: &ModelPack,
    aliases: &[Id],
    declared: &dre_state::ontology::PropertyType,
    state: &CanonicalState,
) -> CarryBack {
    let mut carried = CarryBack::default();
    let step = if matches!(declared.kind, PropertyKind::Integer) {
        Step::Whole
    } else {
        Step::Raw
    };

    let Ok(Some(section)) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)
    else {
        return carried;
    };

    for feature in &section.features {
        // The engine's own walker, reused rather than reimplemented. It
        // preserves duplicates, which is what makes this count meaningful.
        let mentioned = expression_subjects(&feature.expression);
        let occurrences = mentioned
            .iter()
            .filter(|id| names_the_same(id, aliases))
            .count();
        let derived = feature.subject.clone();

        match occurrences {
            0 => continue,
            1 => {}
            many => {
                carried.refuse(derived, Why::ReadTwice(many));
                continue;
            }
        }

        // Everything below substitutes into the expression and evaluates it,
        // so it has to use the spelling the feature itself wrote — the type
        // form where the pack used one — not the instance form the survey
        // built.
        let Some(written) = mentioned
            .iter()
            .find(|id| names_the_same(id, aliases))
            .cloned()
        else {
            continue;
        };
        let subject = &written;

        if let Some(operation) = blocking_operation(&feature.expression, subject) {
            carried.refuse(derived, Why::Blocked(operation));
            continue;
        }

        let thresholds = thresholds_for(pack, std::slice::from_ref(&feature.subject));
        if thresholds.is_empty() {
            carried.refuse(derived, Why::NothingComparesTheDerived);
            continue;
        }

        // The guard is deliberately not evaluated. One pack guards each of
        // its features on `X KNOWN`, which is false in the base state
        // precisely because X is the unknown being surveyed, and true in
        // every run that injects a probe. Seeding blind and letting the run
        // decide is the only policy that gets both directions right — a
        // guard the probe makes false yields NOT_APPLICABLE, which is
        // undecidable, so no flip is reported.
        let (rising, offset) = match line(&feature.expression, subject, state) {
            Ok(found) => found,
            Err(why) => {
                carried.refuse(derived, why);
                continue;
            }
        };

        for (operator, operand) in thresholds {
            let Value::Fixed(threshold) = operand else {
                carried.refuse(
                    derived.clone(),
                    Why::OperandNotFixed(render_value(&operand)),
                );
                continue;
            };

            let at = match boundary(rising, offset, threshold) {
                Ok(at) => at,
                Err(why) => {
                    carried.refuse(derived.clone(), why);
                    continue;
                }
            };

            if matches!(step, Step::Whole)
                && matches!(operator, Operator::Eq | Operator::Ne)
                && at.raw().rem_euclid(Fixed::SCALE) != 0
            {
                let low = at.raw().div_euclid(Fixed::SCALE);
                carried.refuse(
                    derived.clone(),
                    Why::EqualityOffGrid(low, low.saturating_add(1)),
                );
                continue;
            }

            let points = match grid(at, step) {
                Ok(points) => points,
                Err(why) => {
                    carried.refuse(derived.clone(), why);
                    continue;
                }
            };

            // Admissibility first, then the crossing check, so the check can
            // never license a set the filter then strips. Filter, never
            // clamp: an out-of-range reading being admitted is a defect this
            // repository has already had once.
            let admissible: Vec<Value> = points
                .into_iter()
                .filter(|value| declared.in_domain(value))
                .collect();
            if admissible.len() < 2 {
                carried.refuse(derived.clone(), Why::OutsideDomain(declared.domain()));
                continue;
            }

            // The pack's own arithmetic must change its answer across the
            // admissible readings. Pure, in memory, and free.
            let mut sides: Vec<bool> = Vec::new();
            for reading in &admissible {
                let Some(as_fixed) = numeric(reading) else {
                    break;
                };
                let Ok(value) = value_at(&feature.expression, subject, as_fixed, state) else {
                    break;
                };
                let Some(side) = side_of(value, operator, threshold) else {
                    break;
                };
                sides.push(side);
            }
            if sides.len() != admissible.len()
                || !(sides.iter().any(|side| *side) && sides.iter().any(|side| !*side))
            {
                carried.refuse(derived.clone(), Why::SeedDidNotCross);
                continue;
            }

            carried.values.extend(admissible);
            carried.checks.push(Carried {
                derived: derived.clone(),
                operator,
                threshold: Value::Fixed(threshold),
            });
        }
    }

    carried
}

/// A literal, for a diagnostic.
fn render_value(value: &Value) -> String {
    match value {
        Value::Integer(inner) => inner.to_string(),
        Value::Fixed(inner) => inner.to_string(),
        Value::Boolean(inner) => inner.to_string(),
        Value::Symbol(inner) | Value::Reference(inner) => inner.to_string(),
    }
}

/// Why nothing could be carried back, in this repository's house style: say
/// where the value went, name the workaround where there is one.
fn render_refusals(kind: PropertyKind, refusals: &[(Id, Why)]) -> String {
    let kind = kind.as_str();
    let mut parts: Vec<String> = Vec::new();
    for (derived, why) in refusals {
        parts.push(match why {
            Why::Blocked(operation @ ("multiply" | "divide")) => format!(
                "{kind} the pack reads only to compute {derived}, and the route to it \
                 runs through `{operation}`, which truncates toward zero — a boundary \
                 carried back through it can land on the wrong side of the threshold \
                 it was built to sit on"
            ),
            Why::Blocked(operation @ ("min" | "max")) => format!(
                "{kind} the pack reads only to compute {derived}, and the route to it \
                 runs through `{operation}`, which several readings share, so a \
                 threshold on {derived} does not name one value to go and measure"
            ),
            Why::Blocked(operation) => format!(
                "{kind} the pack reads only to compute {derived}, and the route to it \
                 runs through `{operation}`, which names two readings for one \
                 threshold — choosing one of them would be a guess"
            ),
            Why::ReadTwice(count) => format!(
                "{kind} whose expression for {derived} reads it {count} times, and \
                 carrying a threshold back through more than one occurrence is solving \
                 for a variable rather than undoing an operation"
            ),
            Why::SiblingUnreadable(ids) => {
                let named = ids.iter().map(Id::as_str).collect::<Vec<_>>().join(", ");
                format!(
                    "{kind} the pack reads only to compute {derived}, which also reads \
                     {named}; measure those first and this becomes probeable"
                )
            }
            Why::SiblingUnusable(why) => format!(
                "{kind} the pack reads only to compute {derived}, which does not \
                 evaluate here ({why}), so there is no boundary to carry back"
            ),
            Why::SlopeNotUnit => format!(
                "{kind} the pack reads only to compute {derived}, and moving it by one \
                 unit does not move {derived} by one unit, so a threshold on {derived} \
                 does not fix a reading for it"
            ),
            Why::Overflow => format!(
                "{kind} the pack reads only to compute {derived}, and carrying that \
                 threshold back overflows fixed-point before it reaches a reading"
            ),
            Why::OperandNotFixed(text) => format!(
                "{kind} whose threshold on {derived} is compared against `{text}` as a \
                 whole number; a feature always writes fixed-point, so that comparison \
                 is undecidable at run time — write `{{fixed: \"{text}\"}}` and both the \
                 rule and this probe start working"
            ),
            Why::EqualityOffGrid(low, high) => format!(
                "{kind} whose threshold on {derived} falls between {low} and {high} and \
                 is tested for equality, so no whole number sits on it"
            ),
            Why::OutsideDomain(domain) => format!(
                "{kind} declared over {domain}, which holds no pair of readings either \
                 side of {derived}'s threshold"
            ),
            Why::SeedDidNotCross => format!(
                "{kind} whose admissible readings either side of {derived}'s threshold \
                 do not change the pack's answer, so what was carried back is not a \
                 boundary"
            ),
            Why::NothingComparesTheDerived => format!(
                "{kind} the pack reads only to compute {derived}, and nothing compares \
                 {derived} against anything either, so there is no threshold to carry \
                 back"
            ),
        });
    }
    parts.join("; ")
}

/// Why carried probes were withheld after the engine ran them.
fn unconfirmed_reason(kind: PropertyKind, unconfirmed: &[&Carried]) -> String {
    let named = unconfirmed
        .iter()
        .map(|check| check.derived.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} whose carried probes did not change {named}'s side of its threshold when \
         the engine ran them, so they were withheld rather than offered unchecked",
        kind.as_str()
    )
}

/// Every value the ontology admits for a property, when that is a finite set.
fn candidates(
    ontology: &OntologySection,
    type_id: &Id,
    property: &Id,
    pack: &ModelPack,
    aliases: &[Id],
    state: &CanonicalState,
) -> Result<Probes, String> {
    let entity_type = ontology
        .entities
        .get(type_id)
        .ok_or_else(|| format!("the ontology declares no type `{type_id}`"))?;

    let declared = entity_type
        .properties
        .get(property)
        .ok_or_else(|| format!("`{type_id}` declares no property `{property}`"))?;

    match declared.kind {
        PropertyKind::Boolean => Ok(Probes::plain(
            vec![Value::Boolean(true), Value::Boolean(false)],
            declared.kind,
        )),
        PropertyKind::Symbol => match &declared.values {
            Some(vocabulary) if !vocabulary.is_empty() => Ok(Probes::plain(
                vocabulary
                    .iter()
                    .map(|id| Value::Symbol(id.clone()))
                    .collect(),
                declared.kind,
            )),
            // An open vocabulary is unbounded in the same way an unbounded
            // integer is. Declaring `values:` is what closes it.
            _ => Err("symbol with no declared vocabulary, so its values are open".to_owned()),
        },
        PropertyKind::Integer | PropertyKind::Fixed => {
            let direct = numeric_probes(pack, aliases, declared.kind);
            let carried = carry_back(pack, aliases, declared, state);

            let mut values = direct.clone();
            values.extend(carried.values.iter().cloned());
            ordered(&mut values);

            if !values.is_empty() {
                return Ok(Probes {
                    values,
                    direct,
                    kind: declared.kind,
                    checks: carried.checks,
                });
            }

            if feeds_derived(pack, aliases).is_empty() {
                Err(format!(
                    "{} that the pack never compares against anything, so it has \
                     no threshold worth probing",
                    declared.kind.as_str()
                ))
            } else {
                // Every feature reading this property either produced a probe
                // or recorded why it could not, so this is never empty here.
                Err(render_refusals(declared.kind, &carried.refusals))
            }
        }
        PropertyKind::Reference => {
            Err("reference, whose admissible targets are not declared".to_owned())
        }
    }
}

/// Every property of an entity that could still be measured.
///
/// **The ontology says what is measurable; the experiment says what has been
/// measured.** Reading only the experiment's own property list was the
/// obvious implementation and it is the wrong question — it asks "which of
/// the unknowns you declared could change the decision" when a diagnostician
/// asks "what could I measure next".
///
/// The difference is not academic. `internal-web-outage` declares **no**
/// properties at all — everything arrives as evidence — so the survey walked
/// an empty list and reported "there is no observation left to take" on a
/// case where two accounts sat 0.50 apart and the ontology declared
/// twenty-six measurable properties. That is the central step of the
/// diagnostic loop failing on a real pack, and failing silently, in the
/// reassuring direction.
///
/// A property is a candidate when its type declares it and the run did not
/// arrive at a value for it. `CONFLICTING` and `INVALID` are deliberately not
/// candidates: something was measured and the result was unusable, which is a
/// different problem from not having looked, and re-measuring is not
/// obviously the answer to it.
fn candidates_for(
    ontology: &OntologySection,
    entity: &crate::experiment::EntityDecl,
    state: &dre_state::CanonicalState,
) -> Vec<Id> {
    let Some(declared) = ontology.entities.get(&entity.type_id) else {
        return Vec::new();
    };

    declared
        .properties
        .keys()
        .filter(|property| {
            let settled = state
                .entities
                .get(&entity.id)
                .and_then(|found| found.properties.get(*property))
                .is_some_and(|found| !matches!(found.status, dre_state::EpistemicStatus::Unknown));
            !settled
        })
        .cloned()
        .collect()
}

/// Rank the experiment's unknowns by what measuring one would change.
///
/// # Errors
///
/// Fails if the pack has no ontology to read candidates from, or if a run
/// fails.
pub fn survey(
    experiment: &Experiment,
    pack: &ModelPack,
    pack_dir: &std::path::Path,
) -> Result<Survey, CounterfactualError> {
    let ontology = parse_section::<OntologySection>(pack, SectionKind::Ontology)
        .map_err(|error| CounterfactualError::Run(error.into()))?
        .unwrap_or_default();

    let base = crate::runner::run_experiment(experiment, pack_dir)?;
    let derived = derived_subjects(pack, experiment);

    // Fixed once, from the run as written, and then held for every
    // counterfactual below.
    let base_leader = base
        .ranking
        .as_ref()
        .and_then(|ranking| ranking.scored.first())
        .map(|scored| scored.hypothesis.clone());
    let base_lead = base
        .ranking
        .as_ref()
        .zip(base_leader.as_ref())
        .and_then(|(ranking, leader)| ranking.lead_of(leader));

    let mut surveyed = Vec::new();
    let mut declined = Vec::new();

    // Entity order follows the experiment file, and property order follows
    // the entity's declaration. Both are then re-sorted below, so the report
    // does not depend on how the experiment happened to be written.
    for entity in &experiment.given.entities {
        for property in candidates_for(&ontology, entity, &base.state) {
            let subject = match Id::new(format!("{}.{}", entity.id, property)) {
                Ok(subject) => subject,
                Err(_) => continue,
            };

            // A property the pack computes is not one anybody can go and
            // read off an instrument. Skipped silently rather than listed
            // as declined: this is not a limitation of the survey, it is
            // the pack saying where the value comes from.
            if derived.contains(&subject) {
                continue;
            }

            let aliases = aliases_of(&entity.id, &entity.type_id, &property);
            let probes = match candidates(
                &ontology,
                &entity.type_id,
                &property,
                pack,
                &aliases,
                &base.state,
            ) {
                Ok(probes) => probes,
                Err(reason) => {
                    declined.push(NotSurveyable { subject, reason });
                    continue;
                }
            };

            let mut outcomes = Vec::new();
            // One row per carried threshold, recording which side the engine
            // actually landed on at each probe.
            let mut sides: Vec<Vec<Truth>> = vec![Vec::new(); probes.checks.len()];

            for value in probes.values {
                let perturbation = Perturbation {
                    subject: subject.clone(),
                    value: Some(value.clone()),
                    // The ontology declares it, so asking what it would tell
                    // us is a real question rather than a typo.
                    may_introduce: true,
                };
                let comparison =
                    compare(experiment, pack_dir, std::slice::from_ref(&perturbation))?;

                // Asked of the run, not of a model of the run. This is the
                // whole correctness argument: soundness does not rest on the
                // inversion, the slope measurement or the grid policy being
                // right. An error in any of them yields probes that fail here
                // and are withheld, so the worst outcome of a bug is a false
                // decline — the direction this repository demands.
                for (index, check) in probes.checks.iter().enumerate() {
                    let condition = Condition::Compare {
                        subject: check.derived.clone(),
                        operator: check.operator,
                        operand: check.threshold.clone(),
                    };
                    let truth = dre_symbolic::evaluate(&condition, &comparison.alternative.state)
                        .unwrap_or(Truth::Unknown);
                    if let Some(row) = sides.get_mut(index) {
                        row.push(truth);
                    }
                }

                let ranking = comparison.alternative.ranking.as_ref();
                outcomes.push(Outcome {
                    value,
                    decision_changed: comparison.conclusion_changed(),
                    plan_cost: comparison.alternative.plan.as_ref().map(|plan| plan.cost),
                    plan_steps: comparison
                        .alternative
                        .plan
                        .as_ref()
                        .map(|plan| plan.actions.len()),
                    leader: ranking
                        .and_then(|ranked| ranked.scored.first())
                        .map(|scored| scored.hypothesis.clone()),
                    lead: ranking
                        .zip(base_leader.as_ref())
                        .and_then(|(ranked, leader)| ranked.lead_of(leader)),
                });
            }

            // A carried threshold is confirmed only if some probe put the
            // engine on each side of it. `Unknown` is not a side.
            let unconfirmed: Vec<&Carried> = probes
                .checks
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    !sides.get(*index).is_some_and(|row| {
                        row.contains(&Truth::True) && row.contains(&Truth::False)
                    })
                })
                .map(|(_, check)| check)
                .collect();

            if !unconfirmed.is_empty() {
                // Withhold the carried probes rather than offer them
                // unchecked, and fall back to whatever a threshold on the
                // property itself justified.
                outcomes.retain(|outcome| probes.direct.contains(&outcome.value));
                if outcomes.is_empty() {
                    declined.push(NotSurveyable {
                        subject,
                        reason: unconfirmed_reason(probes.kind, &unconfirmed),
                    });
                    continue;
                }
            }

            surveyed.push(Surveyed { subject, outcomes });
        }
    }

    // Most informative first: more reachable decisions, then the cheapest
    // outcome any answer could unlock. Without the second key `haulage`
    // ranks the ford above the bridge on an alphabetical tie-break, when
    // knowing the bridge's rating can save five and knowing the ford's can
    // only cost two.
    //
    // The final tie-break on name is what keeps two runs of the same survey
    // in the same order. The ranking keys alone are not total, and an
    // unstable report about a deterministic engine would be its own small
    // joke.
    surveyed.sort_by(|left, right| {
        // Discrimination leads where a pack declares hypotheses, because
        // "which measurement settles the diagnosis" is a sharper question
        // than "which measurement changes something". Packs with no
        // explanation layer score `None` on every entry, so this key is
        // constant for them and the older ordering below is untouched.
        let separation = |entry: &Surveyed| {
            (
                entry.flips_leader(),
                entry
                    .discrimination()
                    .unwrap_or(dre_core::fixed::Fixed::ZERO),
            )
        };
        separation(right)
            .cmp(&separation(left))
            .then_with(|| right.distinct_decisions().cmp(&left.distinct_decisions()))
            .then_with(|| match (left.best_cost(), right.best_cost()) {
                (Some(left_cost), Some(right_cost)) => left_cost.cmp(&right_cost),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| left.subject.cmp(&right.subject))
    });
    declined.sort_by(|left, right| left.subject.cmp(&right.subject));

    Ok(Survey {
        base_decision: base.record.decision,
        base_cost: base.plan.as_ref().map(|plan| plan.cost),
        base_leader,
        base_lead,
        surveyed,
        declined,
    })
}
