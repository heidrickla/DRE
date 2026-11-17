//! Pack validation beyond "does it parse".
//!
//! `dre validate` used to load a pack and report its sections and hash,
//! which proves the YAML is well formed and nothing else. The gap that
//! leaves is named in `schemas/README.md`: a pack can be structurally valid
//! and still declare a rule about a property no ontology defines.
//!
//! # Why a missing property is worse than a parse error
//!
//! It does not fail. A condition on a property that does not exist
//! evaluates to `Unknown`, so the rule quietly never fires, and the run
//! produces an ordinary-looking answer that simply left a step out. Every
//! other layer is satisfied: the YAML parses, the rule loads, the
//! evaluation is well defined, and the proof faithfully records that the
//! rule was tried and did not fire.
//!
//! So a one-character typo in a subject is invisible everywhere the engine
//! otherwise checks, and produces exactly the failure this project exists
//! to prevent — a decision quietly made on absent data.
//!
//! # Two strengths of check
//!
//! A pack's subjects name entity **instances**; the ontology describes
//! **types**; and which type an instance has is something only an
//! experiment says. The `logic` pack is why that distinction cannot be
//! waved away — it declares one type, `proposition`, and reasons about
//! `a.holds`, `b.holds` and `c.holds`.
//!
//! - [`pack`] checks what a pack can support alone: every property named
//!   anywhere must be declared on *some* entity type. This catches typos,
//!   which is the common case.
//! - [`experiment`] binds instances to types and then checks each subject
//!   against the type it actually has, plus the declared values against
//!   their declared kinds and domains.
//!
//! Neither runs during an experiment. Enforcing domains at run time would
//! change what the engine concludes, which is a change to reasoning
//! behaviour rather than a validation feature — see
//! `dre_state::ontology`.

use std::fmt;

use dre_core::collections::{DetMap, DetSet};
use dre_core::id::Id;
use dre_model::{parse_section, ModelPack, SectionKind};
use dre_state::ontology::OntologySection;

use crate::experiment::Experiment;
use crate::runner::RunError;

/// How much a finding matters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// The pack is inconsistent with itself. A run will misbehave quietly.
    Error,
    /// Something could not be checked, or is suspicious but permitted.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => f.write_str("error"),
            Self::Warning => f.write_str("warning"),
        }
    }
}

/// One thing validation noticed.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// Where it was found, as `section` or `section: item`.
    pub location: String,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.severity, self.location, self.message)
    }
}

/// Whether any finding is an error.
#[must_use]
pub fn has_errors(findings: &[Finding]) -> bool {
    findings
        .iter()
        .any(|finding| finding.severity == Severity::Error)
}

/// A subject reference, with the section that made it.
struct Reference {
    subject: Id,
    location: String,
}

/// Split `entity.property`, or `None` if it is not dotted.
fn split(subject: &Id) -> Option<(&str, &str)> {
    subject.as_str().rsplit_once('.')
}

/// The declaration of a property, when exactly one entity type declares it.
///
/// Returns `None` when several types declare the same property name under
/// different rules, because without an experiment there is no way to say
/// which one a subject means. The binding pass resolves those; guessing
/// here would produce confident findings about the wrong declaration.
fn sole_declaration<'a>(
    ontology: &'a OntologySection,
    property: &Id,
) -> Option<&'a dre_state::ontology::PropertyType> {
    let declaring = ontology.types_declaring(property);
    match declaring.as_slice() {
        [only] => ontology.property_of(only, property),
        _ => None,
    }
}

/// Properties a feature writes.
///
/// Every feature produces fixed-point, whatever the operation: subtracting
/// two integers yields `Fixed`, not `Integer`. So a feature-written
/// property holds fixed-point at run time regardless of what the ontology
/// declares, and that is the fact the two checks below rest on.
fn feature_written(pack: &ModelPack) -> Result<DetSet<Id>, RunError> {
    let mut found = DetSet::new();
    if let Some(section) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)?
    {
        for feature in &section.features {
            if let Some((_, property_name)) = split(&feature.subject) {
                if let Ok(property) = Id::new(property_name) {
                    found.insert(property);
                }
            }
        }
    }
    Ok(found)
}

/// Every literal a rule compares a property against.
fn comparisons(pack: &ModelPack) -> Result<Vec<(Id, dre_state::Value, String)>, RunError> {
    fn walk(
        condition: &dre_symbolic::Condition,
        at: &str,
        out: &mut Vec<(Id, dre_state::Value, String)>,
    ) {
        match condition {
            dre_symbolic::Condition::Compare {
                subject,
                operand,
                operator,
            } => {
                // Only the ordered and equality comparisons read a value;
                // the status operators do not carry one worth checking.
                if matches!(
                    operator,
                    dre_symbolic::Operator::Eq
                        | dre_symbolic::Operator::Ne
                        | dre_symbolic::Operator::Gt
                        | dre_symbolic::Operator::Gte
                        | dre_symbolic::Operator::Lt
                        | dre_symbolic::Operator::Lte
                ) {
                    out.push((subject.clone(), operand.clone(), at.to_owned()));
                }
            }
            dre_symbolic::Condition::Status { .. } => {}
            // A reach compares against a property whose entity is not known
            // until evaluation, so the subject-and-literal type check this
            // collects cannot be made here. `check_reach` below does what
            // can be checked statically -- that the edges and the read
            // property are declared.
            dre_symbolic::Condition::Reach { .. } => {}
            dre_symbolic::Condition::All { all: nested }
            | dre_symbolic::Condition::Any { any: nested } => {
                for inner in nested {
                    walk(inner, at, out);
                }
            }
            dre_symbolic::Condition::Not { not } => walk(not, at, out),
        }
    }

    let mut found = Vec::new();

    if let Some(section) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)? {
        for rule in &section.rules {
            let at = format!("rules: {}", rule.id);
            for condition in &rule.conditions {
                walk(condition, &at, &mut found);
            }
        }
    }

    if let Some(section) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)?
    {
        for feature in &section.features {
            let at = format!("features: {}", feature.id);
            for condition in &feature.when {
                walk(condition, &at, &mut found);
            }
        }
    }

    if let Some(section) = parse_section::<dre_causal::CausalSection>(pack, SectionKind::Causal)? {
        for action in &section.actions {
            let at = format!("causal: {}", action.id);
            for condition in &action.preconditions {
                walk(condition, &at, &mut found);
            }
        }
    }

    if let Some(section) =
        parse_section::<dre_planner::ConstraintsSection>(pack, SectionKind::Constraints)?
    {
        for constraint in &section.constraints {
            let at = format!("constraints: {}", constraint.id);
            for condition in &constraint.conditions {
                walk(condition, &at, &mut found);
            }
        }
    }

    if let Some(section) = parse_section::<dre_planner::GoalsSection>(pack, SectionKind::Goals)? {
        for goal in &section.goals {
            let at = format!("goals: {}", goal.id);
            for condition in &goal.conditions {
                walk(condition, &at, &mut found);
            }
        }
    }

    Ok(found)
}

/// Whether a comparison between these two kinds can ever be decided.
///
/// Ordered comparison is defined only between like kinds, and an integer
/// against a fixed-point value is *not* false — it is `unknown`. So a rule
/// comparing across the boundary never fires, and the run reports nothing
/// unusual. This is the "rule that can never fire" class that validation
/// exists for, arriving through the numeric type system rather than
/// through a typo.
fn comparable(effective: dre_state::PropertyKind, literal: &dre_state::Value) -> bool {
    use dre_state::{PropertyKind, Value};
    match (effective, literal) {
        (PropertyKind::Integer, Value::Integer(_))
        | (PropertyKind::Fixed, Value::Fixed(_))
        | (PropertyKind::Boolean, Value::Boolean(_))
        | (PropertyKind::Symbol, Value::Symbol(_))
        | (PropertyKind::Reference, Value::Reference(_)) => true,
        // A property whose kind the pack does not pin down cannot be
        // checked, and guessing would produce confident nonsense.
        _ => false,
    }
}

/// Name a value's kind for a diagnostic.
fn kind_of(value: &dre_state::Value) -> &'static str {
    use dre_state::Value;
    match value {
        Value::Integer(_) => "integer",
        Value::Fixed(_) => "fixed",
        Value::Boolean(_) => "boolean",
        Value::Symbol(_) => "symbol",
        Value::Reference(_) => "reference",
    }
}

/// Every symbol a pack writes to a property, with where it was written.
///
/// A typo in a *value* is as invisible at run time as a typo in a subject:
/// a symbol nothing compares equal to makes every rule reading it fail to
/// fire, and the run reports nothing unusual.
fn symbol_writes(pack: &ModelPack) -> Result<Vec<(Id, Id, String)>, RunError> {
    let mut found = Vec::new();

    if let Some(section) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)? {
        for rule in &section.rules {
            for inference in &rule.inferences {
                if let dre_state::Value::Symbol(symbol) = &inference.value {
                    found.push((
                        inference.subject.clone(),
                        symbol.clone(),
                        format!("rules: {}", rule.id),
                    ));
                }
            }
        }
    }

    if let Some(section) = parse_section::<dre_causal::CausalSection>(pack, SectionKind::Causal)? {
        for action in &section.actions {
            for (subject, effect) in &action.effects {
                if let dre_causal::Effect::Set(dre_state::Value::Symbol(symbol)) = effect {
                    found.push((
                        subject.clone(),
                        symbol.clone(),
                        format!("causal: {}", action.id),
                    ));
                }
            }
        }
    }

    Ok(found)
}

/// Every subject the pack's sections reference.
///
/// Collected section by section rather than by walking one merged
/// structure, so a finding can say which file to look in. "Some rule
/// mentions a property that does not exist" is a much weaker report than
/// naming the rule.
fn references(pack: &ModelPack) -> Result<Vec<Reference>, RunError> {
    let mut found = Vec::new();

    let mut push = |subject: &Id, location: String| {
        found.push(Reference {
            subject: subject.clone(),
            location,
        });
    };

    if let Some(section) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)? {
        for rule in &section.rules {
            let at = format!("rules: {}", rule.id);
            for subject in dre_symbolic::referenced_subjects(&rule.conditions) {
                push(&subject, at.clone());
            }
            for inference in &rule.inferences {
                push(&inference.subject, at.clone());
            }
        }
    }

    if let Some(section) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)?
    {
        for feature in &section.features {
            let at = format!("features: {}", feature.id);
            push(&feature.subject, at.clone());
            for subject in dre_symbolic::features::referenced_subjects(&feature.expression) {
                push(&subject, at.clone());
            }
            for subject in dre_symbolic::referenced_subjects(&feature.when) {
                push(&subject, at.clone());
            }
        }
    }

    if let Some(section) = parse_section::<dre_causal::CausalSection>(pack, SectionKind::Causal)? {
        for action in &section.actions {
            let at = format!("causal: {}", action.id);
            for subject in dre_symbolic::referenced_subjects(&action.preconditions) {
                push(&subject, at.clone());
            }
            for subject in action.effects.keys() {
                push(subject, at.clone());
            }
        }
    }

    if let Some(section) =
        parse_section::<dre_planner::ConstraintsSection>(pack, SectionKind::Constraints)?
    {
        for constraint in &section.constraints {
            let at = format!("constraints: {}", constraint.id);
            for subject in dre_symbolic::referenced_subjects(&constraint.conditions) {
                push(&subject, at.clone());
            }
        }
    }

    if let Some(section) = parse_section::<dre_planner::GoalsSection>(pack, SectionKind::Goals)? {
        for goal in &section.goals {
            let at = format!("goals: {}", goal.id);
            for subject in dre_symbolic::referenced_subjects(&goal.conditions) {
                push(&subject, at.clone());
            }
        }
    }

    if let Some(section) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)?
    {
        for observation in &section.observations {
            let at = format!("explanations: {}", observation.id);
            for subject in
                dre_symbolic::referenced_subjects(std::slice::from_ref(&observation.when))
            {
                push(&subject, at.clone());
            }
        }
        // The property the ranking writes is a reference like any other. It
        // was the obvious one to forget, because nothing else in the section
        // looks like a subject, and a typo here would leave the ranking
        // computing correctly and writing into nowhere.
        if let Some(target) = &section.rank_into {
            push(target, "explanations: rank_into".to_owned());
        }
    }

    Ok(found)
}

/// Check every traversal against the declared relationship types.
///
/// What can be settled without an experiment: that each edge is a declared
/// relationship type, that consecutive hops actually connect, and that the
/// property read is declared on the type the traversal ends at.
///
/// Worth doing statically because the runtime failure is quiet. A hop naming
/// an undeclared edge reaches nothing, and a reach that reaches nothing is
/// vacuously true under `every` — so the rule fires, the proof records an
/// empty target list, and the pack concludes something confident from a
/// traversal that went nowhere. That is the shape of every expensive defect
/// in this repository.
fn check_reaches(
    pack: &ModelPack,
    ontology: &OntologySection,
    findings: &mut Vec<Finding>,
) -> Result<(), RunError> {
    let Some(section) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)?
    else {
        return Ok(());
    };

    fn walk<'a>(condition: &'a dre_symbolic::Condition, out: &mut Vec<&'a dre_symbolic::Reach>) {
        match condition {
            dre_symbolic::Condition::Reach { reach, .. } => out.push(reach),
            dre_symbolic::Condition::All { all: nested }
            | dre_symbolic::Condition::Any { any: nested } => {
                for inner in nested {
                    walk(inner, out);
                }
            }
            dre_symbolic::Condition::Not { not } => walk(not, out),
            dre_symbolic::Condition::Compare { .. } | dre_symbolic::Condition::Status { .. } => {}
        }
    }

    for rule in &section.rules {
        let at = format!("rules: {}", rule.id);
        let mut reaches = Vec::new();
        for condition in &rule.conditions {
            walk(condition, &mut reaches);
        }

        for reach in reaches {
            // Follow the declared types hop by hop, so a path that cannot
            // connect is caught rather than silently reaching nothing.
            // Seeded with the anchor's own type where the anchor names one.
            // Leaving it `None` meant the FIRST hop was never checked against
            // where it starts, so a reach anchored on `unit` following an edge
            // that runs from `mount` validated clean and then reached nothing
            // at run time -- which is vacuously true under `every`. This check
            // claimed to catch exactly that and did not.
            let mut current: Option<Id> = ontology
                .entities
                .contains_key(&reach.from)
                .then(|| reach.from.clone());
            let mut broken = false;

            for hop in &reach.path {
                let Some(declared) = ontology.relationships.get(&hop.edge) else {
                    findings.push(Finding {
                        severity: Severity::Error,
                        location: at.clone(),
                        message: format!(
                            "reach follows `{}`, which the ontology declares as no relationship",
                            hop.edge
                        ),
                    });
                    broken = true;
                    break;
                };

                let (from, to) = match hop.direction {
                    dre_symbolic::Direction::Out => (&declared.from, &declared.to),
                    dre_symbolic::Direction::In => (&declared.to, &declared.from),
                };

                if let Some(expected) = &current {
                    if expected != from {
                        findings.push(Finding {
                            severity: Severity::Error,
                            location: at.clone(),
                            message: format!(
                                "reach arrives at {expected} and then follows `{}`,                                  which runs from {from}",
                                hop.edge
                            ),
                        });
                        broken = true;
                        break;
                    }
                }
                current = Some(to.clone());
            }

            if broken {
                continue;
            }

            let Some(arrived) = current else { continue };
            if ontology.property_of(&arrived, &reach.read).is_none() {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: at.clone(),
                    message: format!(
                        "reach reads `{}` on {arrived}, which declares no such property",
                        reach.read
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Check the explanation section against itself and against the ontology.
///
/// Two questions the general reference pass cannot ask. Whether the section
/// is internally consistent -- no repeated identifier, no mechanism
/// explaining an observation nobody declared -- is `ExplanationsSection`'s
/// own business, and is asked here so a pack is told at validation time
/// rather than at the first run.
///
/// The second is whether a hypothesis identifier is a value the target
/// property may hold. A ranking writes the winning hypothesis's identifier
/// into `rank_into`, so a hypothesis called `POWER_FAULT` against a property
/// whose vocabulary says `POWER` produces a state the ontology forbids --
/// and produces it only in the runs where that hypothesis happens to win,
/// which is the worst way to find out.
fn check_explanations(
    pack: &ModelPack,
    ontology: &OntologySection,
    findings: &mut Vec<Finding>,
) -> Result<(), RunError> {
    let Some(section) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)?
    else {
        return Ok(());
    };

    if let Err(error) = section.validate() {
        findings.push(Finding {
            severity: Severity::Error,
            location: "explanations".to_owned(),
            message: error.to_string(),
        });
    }

    if section.hypotheses.len() < 2 {
        // Not an error. A single hypothesis still scores, and its score
        // still says whether the state supports it -- which is a real
        // question. But a ranking of one has nothing to rank, and a pack
        // that meant to declare a competitor should hear about it.
        findings.push(Finding {
            severity: Severity::Warning,
            location: "explanations".to_owned(),
            message: format!(
                "declares {} hypotheses, so there is no competing account to weigh against",
                section.hypotheses.len()
            ),
        });
    }

    let Some(target) = &section.rank_into else {
        return Ok(());
    };
    let Some((_, property_name)) = split(target) else {
        return Ok(());
    };
    let Ok(property) = Id::new(property_name) else {
        return Ok(());
    };
    let Some(declared) = sole_declaration(ontology, &property) else {
        return Ok(());
    };

    if declared.kind != dre_state::PropertyKind::Symbol {
        findings.push(Finding {
            severity: Severity::Error,
            location: "explanations: rank_into".to_owned(),
            message: format!(
                "{target} is declared {}, and a ranking writes a hypothesis identifier,                  which is a symbol",
                declared.kind.as_str()
            ),
        });
        return Ok(());
    }

    for hypothesis in &section.hypotheses {
        if !declared.admits(&dre_state::Value::Symbol(hypothesis.id.clone())) {
            findings.push(Finding {
                severity: Severity::Error,
                location: format!("explanations: {}", hypothesis.id),
                message: format!(
                    "{} is not in the vocabulary declared for {target} ({})",
                    hypothesis.id,
                    declared.vocabulary()
                ),
            });
        }
    }

    Ok(())
}

/// Check the geometry section against the ontology and against itself.
///
/// Geometry names bare property identifiers rather than dotted subjects —
/// the space declares its subject once — so it needs its own pass.
fn check_geometry(
    pack: &ModelPack,
    ontology: &OntologySection,
    findings: &mut Vec<Finding>,
) -> Result<(), RunError> {
    let Some(section) =
        parse_section::<dre_geometry::GeometrySection>(pack, SectionKind::Geometry)?
    else {
        return Ok(());
    };

    let declared: DetSet<Id> = section
        .space
        .dimensions
        .iter()
        .map(|dimension| dimension.id.clone())
        .collect();

    for dimension in &section.space.dimensions {
        if !ontology.declares_property(&dimension.id) {
            findings.push(Finding {
                severity: Severity::Error,
                location: "geometry: space".to_owned(),
                message: format!(
                    "dimension {} is not a property any entity type declares",
                    dimension.id
                ),
            });
        }
    }

    if ontology.declares_property(&section.space.classify_into) {
        // Region identifiers *are* the symbols this property comes to
        // hold, so a declared vocabulary for it can be checked against the
        // regions exactly. This is the one place the two declarations have
        // to agree and nothing else would notice them drifting: adding a
        // region without extending the vocabulary produces a
        // classification the ontology says is impossible.
        if let Some(declared) = sole_declaration(ontology, &section.space.classify_into) {
            for region in &section.regions {
                if !declared.admits(&dre_state::Value::Symbol(region.id.clone())) {
                    findings.push(Finding {
                        severity: Severity::Error,
                        location: "geometry: space".to_owned(),
                        message: format!(
                            "region {} is not in the vocabulary declared for {} ({})",
                            region.id,
                            section.space.classify_into,
                            declared.vocabulary()
                        ),
                    });
                }
            }
        }
    } else {
        findings.push(Finding {
            severity: Severity::Error,
            location: "geometry: space".to_owned(),
            message: format!(
                "classify_into writes {}, which is not a property any entity type declares",
                section.space.classify_into
            ),
        });
    }

    // A region naming an axis the space does not have is silently ignored
    // by distance ranking, so it contributes nothing and looks like it
    // does. That is the same class of error as a missing property.
    for region in &section.regions {
        for axis in region.centroid.keys() {
            if !declared.contains(axis) {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: format!("geometry: {}", region.id),
                    message: format!("centroid names {axis}, which the space does not declare"),
                });
            }
        }
        for axis in region.bounds.keys() {
            if !declared.contains(axis) {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: format!("geometry: {}", region.id),
                    message: format!("bounds name {axis}, which the space does not declare"),
                });
            }
        }
        for axis in &declared {
            if !region.centroid.contains_key(axis) {
                findings.push(Finding {
                    severity: Severity::Warning,
                    location: format!("geometry: {}", region.id),
                    message: format!(
                        "centroid does not give a coordinate for {axis}; \
                         distance will treat it as zero"
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Two boolean properties the pack has made mutually exclusive without
/// being able to say so.
///
/// The shape: two rules, each asserting one boolean `true`, each guarded by a
/// single ordered comparison against the *same* subject over ranges that
/// cannot both hold. `latency_high` when a delta is `GTE 25`, `latency_normal`
/// when it is `LTE 10`. The pack plainly knows the two exclude each other,
/// and there is no way to write that down.
///
/// Why it matters, and why it is silent. Concluding `normal` does **not**
/// make `high` false — it leaves it `UNKNOWN`, and an unknown neither
/// supports nor contradicts. So a measurement taken specifically to *rule an
/// account out* cannot rule it out. Nothing reports a problem: the pack
/// loads, validates clean, and concludes confidently.
///
/// This is not hypothetical. An outside author modelling a storage fault
/// predicted a normal reading would leave their storage hypothesis "actively
/// refuted", chose that measurement on those grounds, and the reading moved
/// nothing — the same account led before and after, across every regime of
/// the value they picked. They had flagged the underlying uncertainty
/// themselves and had no way to check it.
///
/// The remedy is one property with both polarities inferred, or a symbol with
/// a closed vocabulary. Both are in `external-pack-brief.md`.
///
/// Deliberately narrow. The general form of this — "an observation that
/// nothing can ever make false" — fires on 85% of the boolean observations in
/// this repository's packs, which makes it noise rather than a finding, and
/// says the format is at fault rather than any author. This tighter shape
/// occurs three times in one pack of twenty-four, and every occurrence is the
/// real defect.
fn exclusive_pairs_never_falsified(pack: &ModelPack) -> Result<Vec<Finding>, RunError> {
    /// The single ordered comparison a rule fires on, if it has exactly one.
    fn sole_guard(
        rule: &dre_symbolic::Rule,
    ) -> Option<(&Id, dre_symbolic::Operator, &dre_state::Value)> {
        let [dre_symbolic::Condition::Compare {
            subject,
            operator,
            operand,
        }] = rule.conditions.as_slice()
        else {
            return None;
        };
        matches!(
            operator,
            dre_symbolic::Operator::Gt
                | dre_symbolic::Operator::Gte
                | dre_symbolic::Operator::Lt
                | dre_symbolic::Operator::Lte
        )
        .then_some((subject, *operator, operand))
    }

    /// Whether these two guards on one subject exclude each other.
    ///
    /// Only the unambiguous case: one bounds from below, the other from
    /// above, and the upper bound does not reach the lower. Anything needing
    /// an argument about `GT` versus `GTE` at a shared endpoint is left
    /// alone — a validator that has to reason to reach a warning is a
    /// validator that will eventually be wrong out loud.
    fn excludes(
        left: (dre_symbolic::Operator, &dre_state::Value),
        right: (dre_symbolic::Operator, &dre_state::Value),
    ) -> bool {
        use dre_symbolic::Operator as Op;
        let (lower, upper) = match (left.0, right.0) {
            (Op::Gt | Op::Gte, Op::Lt | Op::Lte) => (left.1, right.1),
            (Op::Lt | Op::Lte, Op::Gt | Op::Gte) => (right.1, left.1),
            _ => return false,
        };
        match (lower, upper) {
            (dre_state::Value::Integer(low), dre_state::Value::Integer(high)) => high <= low,
            (dre_state::Value::Fixed(low), dre_state::Value::Fixed(high)) => high <= low,
            _ => false,
        }
    }

    let Some(section) = parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)?
    else {
        return Ok(Vec::new());
    };

    // Anything a rule can assert false is not at risk: something can
    // contradict it.
    let falsifiable: DetSet<Id> = section
        .rules
        .iter()
        .flat_map(|rule| &rule.inferences)
        .filter(|inference| inference.value == dre_state::Value::Boolean(false))
        .map(|inference| inference.subject.clone())
        .collect();

    // Rules that assert exactly one boolean true on exactly one threshold.
    let setters: Vec<(&Id, &Id, &Id, dre_symbolic::Operator, &dre_state::Value)> = section
        .rules
        .iter()
        .filter_map(|rule| {
            let [inference] = rule.inferences.as_slice() else {
                return None;
            };
            if inference.value != dre_state::Value::Boolean(true) {
                return None;
            }
            let (subject, operator, operand) = sole_guard(rule)?;
            Some((&rule.id, &inference.subject, subject, operator, operand))
        })
        .collect();

    let mut findings = Vec::new();
    for (index, (rule_a, wrote_a, read_a, op_a, at_a)) in setters.iter().enumerate() {
        for (rule_b, wrote_b, read_b, op_b, at_b) in setters.iter().skip(index + 1) {
            if wrote_a == wrote_b || read_a != read_b {
                continue;
            }
            if falsifiable.contains(*wrote_a) || falsifiable.contains(*wrote_b) {
                continue;
            }
            if !excludes((*op_a, at_a), (*op_b, at_b)) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Warning,
                location: format!("rules: {rule_a} and {rule_b}"),
                message: format!(
                    "{wrote_a} and {wrote_b} cannot both hold, being set on \
                     ranges of {read_a} that exclude each other, but no rule \
                     ever infers either false — so concluding one leaves the \
                     other UNKNOWN rather than false, and an unknown neither \
                     supports nor contradicts. Nothing can refute an account \
                     these appear under. Use one property with both polarities \
                     inferred, or a symbol with a closed vocabulary"
                ),
            });
        }
    }

    Ok(findings)
}

/// An account nothing can ever argue against.
///
/// A hypothesis loses score in exactly one way: `CONTRADICTION`, which needs
/// an observation it explains to evaluate **false**. So an account every one
/// of whose observations is one-way — nothing the pack declares can make the
/// subject false — can gain score and never lose it. No measurement anywhere
/// will rule it out, and a diagnosis that cannot be wrong is not a diagnosis.
///
/// This sits at the **hypothesis** level deliberately. The observation-level
/// version of the same question — "can anything falsify this observation" —
/// holds of 85% of the boolean observations in this repository, which makes
/// it noise rather than a finding, and says the fault is in the format rather
/// than in any author. An account needs only *one* refutable observation, and
/// most have one, so the sharper question stays rare: five accounts of
/// forty-four, in two packs of twenty-six, and both are the pack that
/// demonstrated the defect and its repaired variant.
///
/// What counts as refutable, and why each case:
///
/// - **No rule writes the subject.** It is an input, and evidence can supply
///   either answer. Refutable.
/// - **A rule infers it false somewhere.** Refutable, by construction.
/// - **Rules only ever infer one value.** Not refutable: concluding the
///   opposite state leaves this one `UNKNOWN`, and an unknown neither
///   supports nor contradicts.
/// - **A symbol with more than one admissible value.** Refutable, because
///   concluding a different symbol makes an equality test false.
/// - **Anything numeric, or a condition this cannot read.** Assumed
///   refutable. Over-reporting an account as sound is the safe direction for
///   a check whose whole purpose is to be rare enough to read.
fn accounts_nothing_can_refute(pack: &ModelPack) -> Result<Vec<Finding>, RunError> {
    let Some(section) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)?
    else {
        return Ok(Vec::new());
    };

    let ontology = parse_section::<OntologySection>(pack, SectionKind::Ontology)?;
    let rules =
        parse_section::<dre_symbolic::RulesSection>(pack, SectionKind::Rules)?.unwrap_or_default();

    // Every value any rule writes to a subject. One entry means the subject
    // has a single reachable state.
    let mut written: DetMap<Id, DetSet<dre_state::Value>> = DetMap::new();
    for rule in &rules.rules {
        for inference in &rule.inferences {
            written
                .entry(inference.subject.clone())
                .or_default()
                .insert(inference.value.clone());
        }
    }

    let refutable = |condition: &dre_symbolic::Condition| -> bool {
        let dre_symbolic::Condition::Compare { subject, .. } = condition else {
            // A combining form, a status test or a traversal. Not read here;
            // assumed refutable so the check stays quiet.
            return true;
        };
        let Some(values) = written.get(subject) else {
            return true; // an input, so evidence decides it
        };
        if values.len() > 1 {
            return true;
        }
        let Some((_, property)) = split(subject) else {
            return true;
        };
        let Some(declared) = ontology.as_ref().and_then(|found| {
            Id::new(property)
                .ok()
                .and_then(|id| sole_declaration(found, &id))
        }) else {
            return true;
        };
        match declared.kind {
            // One reachable state, and the other is UNKNOWN rather than
            // false.
            dre_state::PropertyKind::Boolean => false,
            dre_state::PropertyKind::Symbol => declared
                .values
                .as_ref()
                .is_none_or(|vocabulary| vocabulary.len() > 1),
            _ => true,
        }
    };

    let mut can_be_false: DetMap<Id, bool> = DetMap::new();
    for observation in &section.observations {
        can_be_false.insert(observation.id.clone(), refutable(&observation.when));
    }

    let mut findings = Vec::new();
    for hypothesis in &section.hypotheses {
        let explained: Vec<&Id> = hypothesis
            .mechanisms
            .iter()
            .flat_map(|mechanism| &mechanism.explains)
            .collect();
        if explained.is_empty() {
            continue;
        }
        if explained
            .iter()
            .any(|name| can_be_false.get(*name).copied().unwrap_or(true))
        {
            continue;
        }
        findings.push(Finding {
            severity: Severity::Warning,
            location: format!("explanations: {}", hypothesis.id),
            message: "nothing can refute this account — every observation it explains \
                      is one that no rule ever infers false, so it can gain score and \
                      never lose it, and no measurement will rule it out. An account \
                      is contradicted only by an observation that is definitely *not* \
                      happening; give at least one of its observations a rule that can \
                      conclude the negative"
                .to_owned(),
        });
    }

    Ok(findings)
}

/// Validate a pack against itself.
///
/// Returns findings sorted, so two runs over the same pack report the same
/// list in the same order — a validator whose output moved between runs
/// would be an odd thing for this project to ship.
pub fn pack(loaded: &ModelPack) -> Result<Vec<Finding>, RunError> {
    let mut findings = Vec::new();

    let Some(ontology) = parse_section::<OntologySection>(loaded, SectionKind::Ontology)? else {
        findings.push(Finding {
            severity: Severity::Warning,
            location: "ontology".to_owned(),
            message: "no ontology declared, so nothing can be checked against it".to_owned(),
        });
        return Ok(findings);
    };

    if ontology.entities.is_empty() {
        findings.push(Finding {
            severity: Severity::Warning,
            location: "ontology".to_owned(),
            message: "declares no entity types".to_owned(),
        });
    }

    findings.extend(exclusive_pairs_never_falsified(loaded)?);
    findings.extend(accounts_nothing_can_refute(loaded)?);

    for reference in references(loaded)? {
        let Some((_, property_name)) = split(&reference.subject) else {
            findings.push(Finding {
                severity: Severity::Error,
                location: reference.location,
                message: format!("{} is not of the form entity.property", reference.subject),
            });
            continue;
        };

        let Ok(property) = Id::new(property_name) else {
            findings.push(Finding {
                severity: Severity::Error,
                location: reference.location,
                message: format!("{} has an unusable property name", reference.subject),
            });
            continue;
        };

        if !ontology.declares_property(&property) {
            findings.push(Finding {
                severity: Severity::Error,
                location: reference.location,
                message: format!(
                    "{} names property {property}, which no entity type declares",
                    reference.subject
                ),
            });
        }
    }

    for (subject, symbol, location) in symbol_writes(loaded)? {
        let Some((_, property_name)) = split(&subject) else {
            continue;
        };
        let Ok(property) = Id::new(property_name) else {
            continue;
        };
        let Some(declared) = sole_declaration(&ontology, &property) else {
            continue;
        };

        if !declared.admits(&dre_state::Value::Symbol(symbol.clone())) {
            findings.push(Finding {
                severity: Severity::Error,
                location,
                message: format!(
                    "writes {symbol} to {subject}, which declares the vocabulary ({})",
                    declared.vocabulary()
                ),
            });
        }
    }

    for (type_id, entity) in &ontology.entities {
        for (name, property) in &entity.properties {
            if property.values.as_ref().is_some_and(Vec::is_empty) {
                findings.push(Finding {
                    severity: Severity::Warning,
                    location: format!("ontology: {type_id}"),
                    message: format!(
                        "{name} declares an empty vocabulary, so no symbol is admissible; \
                         omit `values` to leave it open"
                    ),
                });
            }
        }
    }

    // A feature-written property is fixed-point at run time whatever the
    // ontology says, so a declaration of `integer` is the pack
    // contradicting itself. Left undetected it is silent: rules comparing
    // that property against integer literals never fire.
    let derived = feature_written(loaded)?;
    for property in &derived {
        if let Some(declared) = sole_declaration(&ontology, property) {
            if declared.kind == dre_state::PropertyKind::Integer {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: "features".to_owned(),
                    message: format!(
                        "{property} is written by a feature, so it holds fixed-point at run \
                         time, but the ontology declares it integer; every feature produces \
                         fixed-point, including subtraction of two integers"
                    ),
                });
            }
        }
    }

    for (subject, literal, location) in comparisons(loaded)? {
        let Some((_, property_name)) = split(&subject) else {
            continue;
        };
        let Ok(property) = Id::new(property_name) else {
            continue;
        };

        // What the property actually holds, which is not always what it
        // was declared to hold.
        let effective = if derived.contains(&property) {
            Some(dre_state::PropertyKind::Fixed)
        } else {
            sole_declaration(&ontology, &property).map(|declared| declared.kind)
        };

        let Some(effective) = effective else {
            continue;
        };

        if !comparable(effective, &literal) {
            findings.push(Finding {
                severity: Severity::Error,
                location,
                message: format!(
                    "compares {subject}, which holds {}, against a literal of kind {}; \
                     unlike kinds compare as unknown, so this can never be decided \
                     and the rule will never fire",
                    effective.as_str(),
                    kind_of(&literal)
                ),
            });
        }
    }

    check_geometry(loaded, &ontology, &mut findings)?;
    check_explanations(loaded, &ontology, &mut findings)?;
    check_reaches(loaded, &ontology, &mut findings)?;

    findings.sort();
    findings.dedup();
    Ok(findings)
}

/// Validate a pack together with an experiment that binds it.
///
/// Everything [`pack`] checks, plus what only an instance-to-type binding
/// makes checkable: that each subject's property belongs to the type its
/// entity actually has, and that each declared value matches its declared
/// kind and domain.
pub fn experiment(loaded: &ModelPack, given: &Experiment) -> Result<Vec<Finding>, RunError> {
    let mut findings = pack(loaded)?;

    let Some(ontology) = parse_section::<OntologySection>(loaded, SectionKind::Ontology)? else {
        return Ok(findings);
    };

    // Every subject's ENTITY half, checked against what the experiment
    // actually declares.
    //
    // The property half has always been checked; the entity half was
    // discarded, so `wrongentity.degraded` passed validation as long as
    // *some* type declared `degraded`. That is not a hypothetical typo: the
    // engine reads such a subject as naming nothing, and `Operator::UNKNOWN`
    // is TRUE for a subject that names nothing — so a misspelled entity does
    // not merely fail to fire a rule, it can fire one.
    //
    // The operator is not the defect and must not be changed.
    // `model-packs/process` depends on exactly that reading: R003 tests
    // `pump.degraded UNKNOWN` and `experiments/process-002.yaml` documents
    // `degraded` as absent from the state, "never concluded". "Nothing ever
    // concluded this" is a legitimate and useful thing to test for. What was
    // missing is catching the name that means nothing, and validation is
    // where that belongs, because it needs the experiment to know which
    // entities exist.
    let declared_entities: dre_core::collections::DetSet<Id> = given
        .given
        .entities
        .iter()
        .map(|entity| entity.id.clone())
        .collect();
    let declared_types: dre_core::collections::DetSet<Id> = given
        .given
        .entities
        .iter()
        .map(|entity| entity.type_id.clone())
        .collect();

    for reference in references(loaded)? {
        let Some((entity_name, _)) = split(&reference.subject) else {
            continue;
        };
        let Ok(entity) = Id::new(entity_name) else {
            continue;
        };
        if declared_entities.contains(&entity) || declared_types.contains(&entity) {
            continue;
        }
        findings.push(Finding {
            severity: Severity::Error,
            location: reference.location,
            message: format!(
                "{} names entity `{entity}`, which this experiment declares neither as an instance nor as a type",
                reference.subject
            ),
        });
    }

    // Instance -> declared type.
    let mut types: dre_core::collections::DetMap<Id, Id> = dre_core::collections::DetMap::new();

    for entity in &given.given.entities {
        let at = format!("experiment: {}", entity.id);

        if !ontology.entities.contains_key(&entity.type_id) {
            findings.push(Finding {
                severity: Severity::Error,
                location: at.clone(),
                message: format!(
                    "declares type {}, which the ontology does not define",
                    entity.type_id
                ),
            });
            continue;
        }
        types.insert(entity.id.clone(), entity.type_id.clone());

        for property in &entity.properties {
            let Some(declared) = ontology.property_of(&entity.type_id, &property.name) else {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: at.clone(),
                    message: format!(
                        "sets {}, which type {} does not declare",
                        property.name, entity.type_id
                    ),
                });
                continue;
            };

            let Some(value) = property.value.as_ref() else {
                continue;
            };

            if !declared.kind.accepts(value) {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: at.clone(),
                    message: format!(
                        "{} is declared {} but is given {value:?}",
                        property.name,
                        declared.kind.as_str()
                    ),
                });
            } else if !declared.in_domain(value) {
                // The case `EpistemicStatus::Invalid` was defined for:
                // observed, but outside the declared domain.
                findings.push(Finding {
                    severity: Severity::Error,
                    location: at.clone(),
                    message: format!(
                        "{} is {value:?}, outside its declared domain {}",
                        property.name,
                        declared.domain()
                    ),
                });
            } else if !declared.admits(value) {
                findings.push(Finding {
                    severity: Severity::Error,
                    location: at.clone(),
                    message: format!(
                        "{} is {value:?}, which is not in its declared vocabulary ({})",
                        property.name,
                        declared.vocabulary()
                    ),
                });
            } else if let dre_state::Value::Reference(target) = value {
                // A reference naming nothing was the one value kind nothing
                // checked. `{ref: db-does-not-exist}` validated clean, which
                // is worse here than for a symbol: `reference` is documented
                // as "reference to another entity", so a typo produces a
                // property that reads as a perfectly ordinary pointer at a
                // thing that is not there.
                //
                // A type name is accepted for the same reason a subject may
                // name one: the engine resolves a type to its sole instance.
                if !declared_entities.contains(target) && !declared_types.contains(target) {
                    findings.push(Finding {
                        severity: Severity::Error,
                        location: at.clone(),
                        message: format!(
                            "{} references `{target}`, which this experiment declares \
                             neither as an instance nor as a type",
                            property.name
                        ),
                    });
                }
            }
        }
    }

    // Now that instances have types, re-check the pack's subjects properly.
    for reference in references(loaded)? {
        let Some((entity_name, property_name)) = split(&reference.subject) else {
            continue;
        };
        let (Ok(entity), Ok(property)) = (Id::new(entity_name), Id::new(property_name)) else {
            continue;
        };

        let Some(type_id) = types.get(&entity) else {
            // The experiment need not mention every entity a pack can talk
            // about, so this is not an error on its own.
            continue;
        };

        // A property that exists nowhere was already reported by the pack
        // pass. Reporting it again from this angle would give one typo two
        // voices, and a reader has to work out whether that is one fault or
        // two. This pass only adds what it alone can see: a real property
        // on the wrong type.
        if !ontology.declares_property(&property) {
            continue;
        }

        if ontology.property_of(type_id, &property).is_none() {
            findings.push(Finding {
                severity: Severity::Error,
                location: reference.location,
                message: format!(
                    "{} names property {property} on entity {entity}, \
                     which is of type {type_id} and does not declare it",
                    reference.subject
                ),
            });
        }
    }

    findings.sort();
    findings.dedup();
    Ok(findings)
}
