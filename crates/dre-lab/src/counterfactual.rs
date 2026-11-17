//! Counterfactuals — plan section 26.
//!
//! "What would this engine have decided if one thing had been different?"
//!
//! The first post-MVP track, and it is deliberately the one that adds no
//! reasoning. A counterfactual here is two ordinary runs and a comparison:
//! the base experiment, the same experiment with one property changed, and
//! a diff. Nothing reaches a conclusion that a plain `dre run` could not.
//!
//! That constraint is the point rather than a limitation. The architectural
//! claim is that a new capability should not need a runtime change, and a
//! counterfactual engine that reasoned *about* alternatives — hypothetical
//! states, modal operators, a second inference mode — would be a second
//! engine to keep deterministic. This one borrows the first engine twice.
//!
//! # Why the five hashes already describe it
//!
//! A counterfactual pair shares a `ModelHash` and an `EngineHash` and
//! differs in `InputHash` — the shape replay already reports as "different
//! question, same engine". So the comparison is meaningful by construction:
//! any difference in `DecisionHash` is attributable to the perturbation,
//! because everything else is byte-identical.
//!
//! # It was already being done by hand
//!
//! `haulage-001` and `haulage-002` differ in exactly one property and
//! return different routes; `diagnosis-002` is the same shape. Those pairs
//! are counterfactuals authored as separate experiments, which works and
//! does not scale: it needs a whole second experiment per question, and it
//! reports two independent results rather than what changed between them.

use std::collections::BTreeSet;

use dre_core::id::Id;
use dre_state::{EpistemicStatus, Value};

use crate::experiment::Experiment;
use crate::runner::{self, RunError, RunResult};

/// One change to the input state.
///
/// `value` absent means "make this unknown again" — the reverse direction,
/// and the one that matters for deciding what is worth measuring: it asks
/// what the engine would have concluded *without* an observation it has.
#[derive(Clone, Debug)]
pub struct Perturbation {
    /// `entity.property`.
    pub subject: Id,
    /// The value to set, or `None` to return the property to `UNKNOWN`.
    pub value: Option<Value>,
    /// Whether the property may be added when the experiment does not
    /// declare it.
    ///
    /// **False for anything hand-written.** An experiment declares the
    /// properties it is a question about, and silently inventing one would
    /// let a typo perturb nothing and report "no change" — a wrong answer
    /// that looks exactly like a real finding.
    ///
    /// True only for `survey`, and only for a property the **ontology**
    /// declares. The survey is asking "what if this were measured", and the
    /// ontology is what says the measurement exists — so refusing there
    /// answers a different question from the one being asked. That refusal
    /// is why the diagnostic loop reported "no observation left to take" on
    /// a pack whose ontology declared twenty-six measurable properties.
    pub may_introduce: bool,
}

/// What a counterfactual could not do.
#[derive(Debug)]
pub enum CounterfactualError {
    /// The subject was not `entity.property`.
    MalformedSubject(String),
    /// No entity of that name is declared by the experiment.
    NoSuchEntity(String),
    /// The entity exists but does not declare that property.
    ///
    /// Refused rather than added. An experiment declares the properties it
    /// is a question about, and silently inventing one would let a typo
    /// perturb nothing and report "no change" — which is a wrong answer
    /// that looks exactly like a real finding.
    NoSuchProperty {
        /// The entity that was found.
        entity: String,
        /// The property that was not.
        property: String,
    },
    /// One of the two runs failed.
    Run(RunError),
}

impl std::fmt::Display for CounterfactualError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedSubject(subject) => {
                write!(formatter, "`{subject}` is not `entity.property`")
            }
            Self::NoSuchEntity(entity) => {
                write!(formatter, "the experiment declares no entity `{entity}`")
            }
            Self::NoSuchProperty { entity, property } => write!(
                formatter,
                "`{entity}` declares no property `{property}`; \
                 a counterfactual may change a declared property, not invent one"
            ),
            Self::Run(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for CounterfactualError {}

impl From<RunError> for CounterfactualError {
    fn from(error: RunError) -> Self {
        Self::Run(error)
    }
}

/// One property whose final value differs between the two runs.
#[derive(Debug)]
pub struct PropertyDelta {
    /// `entity.property`.
    pub subject: String,
    /// How it read in the base run.
    pub before: String,
    /// How it read in the counterfactual.
    pub after: String,
}

/// The result of asking one "what if".
#[derive(Debug)]
pub struct Comparison {
    /// The unmodified run.
    pub base: RunResult,
    /// The run with the perturbation applied.
    pub alternative: RunResult,
    /// Properties that ended up different.
    pub deltas: Vec<PropertyDelta>,
    /// Rules that fired in the base run and not the counterfactual.
    pub no_longer_fired: Vec<Id>,
    /// Rules that fired in the counterfactual and not the base run.
    pub newly_fired: Vec<Id>,
}

impl Comparison {
    /// Whether the engine reached a different decision.
    ///
    /// Read off the `DecisionHash` rather than by inspecting the state,
    /// because that hash is what the rest of the system means by "a
    /// different answer" — it covers the plan as well as the state, and a
    /// counterfactual that changed only the route would otherwise report
    /// as unchanged.
    #[must_use]
    pub fn decision_changed(&self) -> bool {
        self.base.record.decision != self.alternative.record.decision
    }

    /// Whether the perturbation reached the engine at all.
    ///
    /// A counterfactual that sets a property to the value it already holds
    /// produces an identical input, and "no change" then says nothing about
    /// the engine's sensitivity — it says the question was not asked. Worth
    /// distinguishing loudly from a real negative result.
    #[must_use]
    pub fn input_changed(&self) -> bool {
        self.base.record.input != self.alternative.record.input
    }

    /// Whether the engine *concluded* anything different.
    ///
    /// This is the question a counterfactual is usually asking, and it is
    /// not [`Self::decision_changed`].
    ///
    /// `DecisionHash` covers the state, and the state contains the
    /// observation the perturbation injected — so it moves whenever the
    /// input moves, whatever the engine went on to think. Measured on
    /// `haulage-001`, every unknown looked decisive: setting
    /// `north_bridge.rating_ok` to `false` reported a changed decision while
    /// returning the identical route at the identical cost, because a
    /// blocked bridge and an unrated one are the same bridge to a planner.
    ///
    /// So this compares what the engine *derived*: the plan, and every
    /// property it inferred. Observations are excluded on both sides,
    /// because an echo of the question is not an answer to it.
    #[must_use]
    pub fn conclusion_changed(&self) -> bool {
        let plans_differ = match (&self.base.plan, &self.alternative.plan) {
            (Some(base), Some(alternative)) => {
                base.actions != alternative.actions || base.cost != alternative.cost
            }
            (None, None) => false,
            _ => true,
        };

        plans_differ || inferred(&self.base) != inferred(&self.alternative)
    }
}

/// Every property the engine derived, as `subject -> reading`.
///
/// `Inferred` only. A `Known` property is something the experiment said,
/// including whatever a perturbation just set, and counting those would
/// make every counterfactual look consequential.
fn inferred(result: &RunResult) -> std::collections::BTreeMap<String, String> {
    let mut found = std::collections::BTreeMap::new();
    for (entity_id, entity) in result.state.entities.iter() {
        for (property_id, property) in entity.properties.iter() {
            if property.status == EpistemicStatus::Inferred {
                found.insert(
                    format!("{entity_id}.{property_id}"),
                    render(property.value.as_ref(), property.status),
                );
            }
        }
    }
    found
}

/// Apply the perturbations to a copy of the experiment's declared state.
fn perturb(
    experiment: &Experiment,
    perturbations: &[Perturbation],
) -> Result<Experiment, CounterfactualError> {
    let mut altered = experiment.clone();

    for perturbation in perturbations {
        let (entity_name, property_name) = perturbation
            .subject
            .as_str()
            .rsplit_once('.')
            .ok_or_else(|| {
                CounterfactualError::MalformedSubject(perturbation.subject.to_string())
            })?;

        let entity = altered
            .given
            .entities
            .iter_mut()
            .find(|entity| entity.id.as_str() == entity_name)
            .ok_or_else(|| CounterfactualError::NoSuchEntity(entity_name.to_owned()))?;

        let existing = entity
            .properties
            .iter_mut()
            .find(|property| property.name.as_str() == property_name);

        let status = Some(match perturbation.value {
            Some(_) => EpistemicStatus::Known,
            None => EpistemicStatus::Unknown,
        });

        match existing {
            Some(property) => {
                property.value = perturbation.value.clone();
                property.status = status;
            }
            None if perturbation.may_introduce => {
                let Ok(name) = Id::new(property_name) else {
                    return Err(CounterfactualError::NoSuchProperty {
                        entity: entity_name.to_owned(),
                        property: property_name.to_owned(),
                    });
                };
                entity.properties.push(crate::experiment::PropertyDecl {
                    name,
                    value: perturbation.value.clone(),
                    status,
                });
            }
            None => {
                return Err(CounterfactualError::NoSuchProperty {
                    entity: entity_name.to_owned(),
                    property: property_name.to_owned(),
                })
            }
        }
    }

    Ok(altered)
}

/// How a property reads, for the diff.
fn render(value: Option<&Value>, status: EpistemicStatus) -> String {
    match value {
        Some(value) => format!("{value:?} [{status:?}]"),
        None => format!("— [{status:?}]"),
    }
}

/// Every `entity.property` in either final state, and how each reads.
fn readings(result: &RunResult) -> std::collections::BTreeMap<String, String> {
    let mut found = std::collections::BTreeMap::new();
    for (entity_id, entity) in result.state.entities.iter() {
        for (property_id, property) in entity.properties.iter() {
            found.insert(
                format!("{entity_id}.{property_id}"),
                render(property.value.as_ref(), property.status),
            );
        }
    }
    found
}

/// Run the experiment, then run it again with the perturbations applied.
///
/// # Errors
///
/// Fails if a perturbation names something the experiment does not declare,
/// or if either run fails.
pub fn compare(
    experiment: &Experiment,
    pack_dir: &std::path::Path,
    perturbations: &[Perturbation],
) -> Result<Comparison, CounterfactualError> {
    let altered = perturb(experiment, perturbations)?;

    let base = runner::run_experiment(experiment, pack_dir)?;
    let alternative = runner::run_experiment(&altered, pack_dir)?;

    let before = readings(&base);
    let after = readings(&alternative);

    let mut subjects: BTreeSet<&String> = BTreeSet::new();
    subjects.extend(before.keys());
    subjects.extend(after.keys());

    let absent = "absent".to_owned();
    let deltas = subjects
        .into_iter()
        .filter_map(|subject| {
            let was = before.get(subject).unwrap_or(&absent);
            let now = after.get(subject).unwrap_or(&absent);
            (was != now).then(|| PropertyDelta {
                subject: subject.clone(),
                before: was.clone(),
                after: now.clone(),
            })
        })
        .collect();

    let base_fired: BTreeSet<&Id> = base.fired.iter().collect();
    let alt_fired: BTreeSet<&Id> = alternative.fired.iter().collect();

    let no_longer_fired = base_fired
        .difference(&alt_fired)
        .map(|id| (*id).clone())
        .collect();
    let newly_fired = alt_fired
        .difference(&base_fired)
        .map(|id| (*id).clone())
        .collect();

    Ok(Comparison {
        base,
        alternative,
        deltas,
        no_longer_fired,
        newly_fired,
    })
}
