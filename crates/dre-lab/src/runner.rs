//! The experiment runner.
//!
//! Composes the engine crates into one pass: load the pack, build the
//! declared state, run the reasoners, and produce a decision plus the five
//! hashes that make the run replayable.
//!
//! This is the seam where the harness meets the engine. The runner may
//! read files and measure elapsed time; nothing it measures is handed back
//! into the reasoning path, so no timing can change an answer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dre_core::canonical::Canonical;
use dre_core::context::ExecutionContext;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use dre_model::{load_dir, parse_section, SectionKind};
use dre_proof::{ProofGraph, Trace};
use dre_state::{CanonicalState, EpistemicStatus, StateBuilder, Value};
use dre_symbolic::RulesSection;

use crate::experiment::{Expect, Experiment, ExperimentFile};

/// Failures running an experiment.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Reading the experiment or pack failed.
    #[error("{path}: {source}")]
    Io {
        /// Path being read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The experiment file did not parse.
    #[error("{path}: {source}")]
    Parse {
        /// Path being parsed.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: Box<serde_saphyr::Error>,
    },

    /// Loading the model pack failed.
    #[error("model pack: {0}")]
    Pack(#[from] dre_model::LoadError),

    /// Building the declared state failed.
    #[error("building state: {0}")]
    State(#[from] dre_state::StateError),

    /// Reasoning failed.
    #[error("reasoning: {0}")]
    Reason(#[from] dre_symbolic::EvalError),

    /// Deriving a declared feature failed.
    #[error("features: {0}")]
    Feature(#[from] dre_symbolic::FeatureError),

    /// The runtime configuration was rejected.
    #[error("runtime: {0}")]
    Core(#[from] dre_core::CoreError),

    /// Geometric classification failed.
    #[error("geometry: {0}")]
    Geometry(String),

    /// Ranking the declared hypotheses failed.
    #[error("explanations: {0}")]
    Explain(#[from] dre_explain::ExplainError),

    /// Search failed or hit a bound.
    #[error("search: {0}")]
    Search(String),

    /// Reading or writing a replay record failed.
    #[error("record: {0}")]
    Store(String),
}

/// Everything one run produced.
#[derive(Debug)]
pub struct RunResult {
    /// The experiment's identifier.
    pub experiment: Id,
    /// State after reasoning.
    pub state: CanonicalState,
    /// The proof.
    pub proof: ProofGraph,
    /// Rules that fired, in order.
    pub fired: Vec<Id>,
    /// Workload counters.
    pub counters: dre_core::context::Counters,
    /// The five hashes of plan section 17.
    pub record: dre_replay::ReplayRecord,
    /// The plan, when the pack declares actions and a goal.
    pub plan: Option<dre_planner::Plan>,
    /// The explanation ranking, when the pack declares hypotheses.
    pub ranking: Option<dre_explain::Ranking>,
    /// Wall-clock duration. Reported only; never observed by the engine.
    pub elapsed_micros: u128,
    /// Assertion failures, empty when the experiment passed.
    pub failures: Vec<String>,
}

impl RunResult {
    /// Whether every declared expectation held.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Load an experiment definition.
pub fn load_experiment(path: &Path) -> Result<Experiment, RunError> {
    let text = std::fs::read_to_string(path).map_err(|source| RunError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let file: ExperimentFile =
        dre_model::yaml::from_str(&text).map_err(|source| RunError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;

    Ok(file.experiment)
}

/// Resolve the pack path relative to the experiment file's directory, then
/// to the current directory.
pub fn resolve_pack(experiment_path: &Path, pack: &Path) -> PathBuf {
    if pack.is_absolute() {
        return pack.to_path_buf();
    }
    if let Some(parent) = experiment_path.parent() {
        let candidate = parent.join(pack);
        if candidate.is_dir() {
            return candidate;
        }
        // experiments/ sits beside model-packs/ at the repository root.
        let sibling = parent.join("..").join(pack);
        if sibling.is_dir() {
            return sibling;
        }
    }
    pack.to_path_buf()
}

/// Build the experiment's declared starting state.
fn build_state(
    experiment: &Experiment,
    context: &ExecutionContext,
    relationship_types: dre_core::collections::DetMap<Id, dre_state::ontology::RelationshipType>,
) -> Result<CanonicalState, RunError> {
    let mut builder =
        StateBuilder::new(context.config().limits).with_relationship_types(relationship_types);

    for entity in &experiment.given.entities {
        builder.entity(entity.id.clone(), entity.type_id.clone())?;
    }

    for entity in &experiment.given.entities {
        for property in &entity.properties {
            match (&property.value, property.effective_status()) {
                (Some(value), EpistemicStatus::Inferred) => {
                    builder.infer(&entity.id, property.name.clone(), value.clone())?;
                }
                (Some(value), _) => {
                    builder.observe(&entity.id, property.name.clone(), value.clone())?;
                }
                (None, status) => {
                    builder.declare(&entity.id, property.name.clone(), status)?;
                }
            }
        }
    }

    // After the entity loop, because `relate` validates endpoints against
    // entities that must already exist.
    for edge in &experiment.given.relationships {
        builder.relate(
            edge.id.clone(),
            edge.type_id.clone(),
            edge.from.clone(),
            edge.to.clone(),
        )?;
    }

    Ok(builder.build())
}

/// Join an entity and a property into the dotted subject the engine uses.
///
/// Returns `None` for a pair that cannot form a valid identifier, in which
/// case the property simply gets no provenance node — the run is unaffected,
/// and a missing edge is preferable to a fabricated one.
fn subject_of(entity: &Id, property: &Id) -> Option<Id> {
    Id::new(format!("{entity}.{property}")).ok()
}

/// Record every declared fact as an input node.
///
/// Without these a rule that reads a declared property has nothing to point
/// at, and the provenance chain starts halfway along — the conclusion traces
/// to the rule and stops. They are emitted from the **built** state rather
/// than from the experiment file, so the order is the canonical one: the
/// same facts declared in a different order in the YAML produce the same
/// proof, which is one of the properties plan section 21 attacks.
fn record_inputs(state: &CanonicalState, context: &mut ExecutionContext, trace: &mut Trace) {
    for (entity_id, entity) in &state.entities {
        for property_id in entity.properties.keys() {
            let Some(subject) = subject_of(entity_id, property_id) else {
                continue;
            };
            trace.establish(
                subject.clone(),
                dre_proof::ProofNode {
                    kind: dre_proof::NodeKind::Input { subject },
                    inputs: Vec::new(),
                },
            );
            context.charge_proof_node();
        }
    }
}

/// Subjects that were `UNKNOWN` in the state as authored.
///
/// The denominator for the unknown-resolution rate. `UNOBSERVABLE` and the
/// other no-value states are excluded: they are statements the model makes
/// on purpose, not gaps waiting to be filled.
fn unknown_subjects(state: &CanonicalState) -> Vec<Id> {
    let mut out = Vec::new();
    for (entity_id, entity) in &state.entities {
        for (property_id, property) in &entity.properties {
            if property.status == EpistemicStatus::Unknown {
                if let Some(subject) = subject_of(entity_id, property_id) {
                    out.push(subject);
                }
            }
        }
    }
    out
}

/// How many of `subjects` are no longer `UNKNOWN` in `state`.
///
/// A subject that vanished from the state counts as unresolved. Treating an
/// absent property as decided would let a run score better by losing track
/// of the question.
fn resolved_count(state: &CanonicalState, subjects: &[Id]) -> u64 {
    let mut resolved = 0u64;
    for subject in subjects {
        let Some((entity_name, property_name)) = subject.as_str().rsplit_once('.') else {
            continue;
        };
        let status = state
            .entities
            .iter()
            .find(|(id, _)| id.as_str() == entity_name)
            .and_then(|(_, entity)| {
                entity
                    .properties
                    .iter()
                    .find(|(id, _)| id.as_str() == property_name)
                    .map(|(_, property)| property.status)
            });
        if let Some(status) = status {
            if status != EpistemicStatus::Unknown {
                resolved = resolved.saturating_add(1);
            }
        }
    }
    resolved
}

/// Fold resolved evidence into the declared state.
///
/// Resolutions are written with [`StateBuilder::set`], not `observe`: the
/// resolver has already weighed the competing claims with provenance the
/// merge rule does not have, so re-merging would discard that work and
/// could collapse a resolved answer back to `CONFLICTING`.
/// Why an incoming observation is not fit to reason with, if it is not.
///
/// Checked against the ontology the pack declares, which is the only place
/// that says what a property may hold. The same three questions
/// `dre validate` asks of a declared property, asked of a reading — which
/// validation never did, because it walks entities and never touches
/// `given.evidence`.
///
/// ⭐ AND A FOURTH QUESTION, added 0.22.0: does the property exist on this
/// entity's type at all? Until then the ontology lookup on the line below
/// used `?`, so a miss returned `None` — which this function spells "no
/// defect". A reading naming a predicate the type does not declare was
/// therefore admitted, resolved, and written into state, and no layer
/// objected: `StateBuilder::set` does not consult the ontology either. The
/// engine then reasons over a property the model never said could exist,
/// which produces a confident answer rather than a failure.
///
/// ⛔ THE TWO MISSES ARE NOT THE SAME MISS, and collapsing them would be a
/// bug. An absent SUBJECT still returns `None` here on purpose: it is caught
/// downstream by `StateBuilder::draft_mut` as `UnknownEntity`, and more
/// importantly `Operator::UNKNOWN` on a subject that names nothing is CORRECT
/// and load-bearing — `model-packs/process` R003 depends on that reading.
/// Only the predicate arm is a defect.
fn defect_in(
    ontology: &dre_state::ontology::OntologySection,
    state: &CanonicalState,
    observation: &dre_evidence::Evidence,
) -> Option<String> {
    // Absent subject: not a defect HERE. See the note above -- this arm is
    // deliberately unchanged.
    let entity = state.entities.get(&observation.subject)?;
    let Some(declared) = ontology.property_of(&entity.type_id, &observation.predicate) else {
        return Some(format!(
            "{} is not declared on type {}",
            observation.predicate, entity.type_id
        ));
    };

    if !declared.kind.accepts(&observation.value) {
        return Some(format!(
            "{} is declared {} and the reading is {:?}",
            observation.predicate,
            declared.kind.as_str(),
            observation.value
        ));
    }
    if !declared.in_domain(&observation.value) {
        return Some(format!(
            "{:?} is outside the declared domain {}",
            observation.value,
            declared.domain()
        ));
    }
    if !declared.admits(&observation.value) {
        return Some(format!(
            "{:?} is not in the declared vocabulary ({})",
            observation.value,
            declared.vocabulary()
        ));
    }
    None
}

fn apply_evidence(
    experiment: &Experiment,
    pack: &dre_model::ModelPack,
    state: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<CanonicalState, RunError> {
    // Attempts that produced nothing are recorded before anything else, so
    // the proof shows the instrument was asked even when the run has no
    // readings at all to resolve.
    //
    // They never reach resolution: they are a separate list, so their
    // exclusion is structural rather than a filter. And the property is
    // left exactly as declared -- a failed read does not make a value
    // INVALID, because no value arrived to be invalid. It stays unobserved,
    // with the reason recorded beside it.
    for attempt in &experiment.given.failed_reads {
        trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::ObservationFailed {
                observation: attempt.id.clone(),
                source: attempt.source.clone(),
                reason: attempt.reason.clone(),
            },
            inputs: Vec::new(),
        });
        context.charge_proof_node();
    }

    if experiment.given.evidence.is_empty() {
        return Ok(state);
    }

    let sources: dre_core::collections::DetMap<Id, dre_evidence::Source> = experiment
        .given
        .sources
        .iter()
        .map(|source| (source.id.clone(), source.clone()))
        .collect();

    // A defective reading is stated and excluded, never repaired. It does
    // not reach resolution at all, because a value that cannot be true
    // must not be allowed to vote — and a garbage reading that cannot win
    // a vote can still spoil one.
    //
    // `device-cascade` is why this exists rather than a hypothetical: a
    // temperature of 9999 against a declared max of 200 was admitted, and
    // its disagreement with a sound reading turned a diagnosis into
    // CONFLICTING. The engine then reached the right answer *because* a
    // sensor was lying, and repairing the sensor broke it.
    let ontology =
        parse_section::<dre_state::ontology::OntologySection>(pack, SectionKind::Ontology)?
            .unwrap_or_default();

    let mut sound: Vec<dre_evidence::Evidence> = Vec::new();
    let mut defective: BTreeSet<Id> = BTreeSet::new();

    for observation in &experiment.given.evidence {
        match defect_in(&ontology, &state, observation) {
            None => sound.push(observation.clone()),
            Some(reason) => {
                // Stated, by name and by source, rather than dropped
                // quietly. A reading discarded without a record is
                // indistinguishable from one that was never taken.
                trace.record(dre_proof::ProofNode {
                    kind: dre_proof::NodeKind::Rejected {
                        candidate: observation.id.clone(),
                        reason: format!("defective reading from {}: {reason}", observation.source),
                    },
                    inputs: Vec::new(),
                });
                context.charge_proof_node();

                if let Ok(subject) =
                    Id::new(format!("{}.{}", observation.subject, observation.predicate))
                {
                    defective.insert(subject);
                }
            }
        }
    }

    let resolutions = dre_evidence::resolve_all(
        &sound,
        &sources,
        &experiment.given.policy,
        experiment.given.now,
    );

    let mut builder = StateBuilder::from_state(state, context.config().limits);

    for resolution in &resolutions {
        let Some((entity_name, property_name)) = resolution.subject.as_str().rsplit_once('.')
        else {
            continue;
        };
        let (Ok(entity), Ok(property)) = (Id::new(entity_name), Id::new(property_name)) else {
            continue;
        };

        builder.set(
            &entity,
            property,
            resolution.value.clone(),
            resolution.status,
        )?;

        // Name the observations that *won*, before the node that rests on
        // them.
        //
        // `Resolution.supporting` has always carried these and nothing
        // read it, so a contested measurement recorded the reading it
        // overruled and collapsed the decisive ones into an anonymous
        // input. The proof kept provenance for the discarded observation
        // and dropped it for the ones that mattered — which is precisely
        // backwards for a diagnosis whose question is which instrument to
        // trust. Found by an outside author, who noticed the symptom
        // rather than the unused field.
        let mut backing = Vec::new();
        for observation in &resolution.supporting {
            let source = sound
                .iter()
                .find(|candidate| &candidate.id == observation)
                .map(|candidate| candidate.source.clone());
            let Some(source) = source else {
                continue;
            };
            backing.push(trace.record(dre_proof::ProofNode {
                kind: dre_proof::NodeKind::Observed {
                    observation: observation.clone(),
                    source,
                },
                inputs: Vec::new(),
            }));
            context.charge_proof_node();
        }

        // The resolution supersedes whatever the state declared, so it
        // becomes the property's provenance: a rule reading this value
        // traces to the resolution, and from there to the sources.
        let resolved = trace.establish(
            resolution.subject.clone(),
            dre_proof::ProofNode {
                kind: dre_proof::NodeKind::Input {
                    subject: resolution.subject.clone(),
                },
                inputs: backing,
            },
        );
        context.charge_proof_node();

        // Record what was overruled. A resolution that showed only its
        // winner would not answer "why not the other reading", which is
        // the question asked of any contested measurement.
        for dissenter in &resolution.dissenting {
            trace.record(dre_proof::ProofNode {
                kind: dre_proof::NodeKind::Rejected {
                    candidate: dissenter.clone(),
                    reason: format!(
                        "overruled resolving {} ({} independent groups backed the result)",
                        resolution.subject, resolution.corroboration
                    ),
                },
                // Pointed at the resolution that overruled it, so the
                // rejection is readable as part of one decision rather
                // than as a loose note beside it.
                inputs: vec![resolved],
            });
            context.charge_proof_node();
        }
    }

    // A subject every one of whose readings was defective becomes INVALID,
    // not UNKNOWN. "We looked and what came back was bad" is strictly more
    // informative than "nobody looked", and rules reading it decline to
    // fire either way.
    //
    // Only where nothing sound survived: if one reading of three was
    // defective, the other two resolve normally and the subject keeps
    // whatever they establish.
    for subject in &defective {
        if resolutions
            .iter()
            .any(|resolution| &resolution.subject == subject)
        {
            continue;
        }
        let Some((entity_name, property_name)) = subject.as_str().rsplit_once('.') else {
            continue;
        };
        let (Ok(entity), Ok(property)) = (Id::new(entity_name), Id::new(property_name)) else {
            continue;
        };
        builder.set(&entity, property, None, EpistemicStatus::Invalid)?;
    }

    Ok(builder.build())
}

/// Run an experiment end to end.
pub fn run(path: &Path) -> Result<RunResult, RunError> {
    let experiment = load_experiment(path)?;
    run_experiment(&experiment, &resolve_pack(path, &experiment.pack))
}

/// Run a loaded experiment against a pack directory.
pub fn run_experiment(experiment: &Experiment, pack_dir: &Path) -> Result<RunResult, RunError> {
    let started = Instant::now();

    let pack = load_dir(pack_dir)?;

    // The experiment may raise the ceilings, and doing so puts it on a
    // different engine. Nothing here treats that as an exception: the
    // config goes into the `EngineHash` exactly as the default would.
    let mut config = dre_core::config::RuntimeConfig::default();
    if let Some(limits) = experiment.limits {
        config.limits = limits;
    }
    if let Some(strategy) = experiment.strategy {
        config.strategy = strategy;
    }
    let mut context = ExecutionContext::new(config)?;
    let mut trace = Trace::new();

    // Parsed once, before the state is built, so declared edges are checked
    // against declared types rather than merely against existence.
    let ontology =
        parse_section::<dre_state::ontology::OntologySection>(&pack, SectionKind::Ontology)?
            .unwrap_or_default();

    refuse_reach_in_goals(&pack)?;

    let declared = build_state(experiment, &context, ontology.relationships.clone())?;
    // Measured on the state as authored, before resolution gets a chance to
    // close any of the gaps — the point of the metric is how many it closed.
    let started_unknown = unknown_subjects(&declared);
    record_inputs(&declared, &mut context, &mut trace);

    let observed = apply_evidence(experiment, &pack, declared, &mut context, &mut trace)?;

    // The input hash covers the state as given to the engine, before any
    // derivation. Hashing after features would fold the engine's own work
    // into the description of the problem, and two engines deriving
    // different features from identical observations would then look like
    // they had been asked different questions.
    let input_hash = observed.canonical_hash();

    let initial = derive_features(&pack, observed, &mut context, &mut trace)?;

    let rules: Vec<dre_symbolic::Rule> = parse_section::<RulesSection>(&pack, SectionKind::Rules)?
        .unwrap_or_default()
        .rules;
    context.check_rule_count(rules.len() as u64)?;

    // Read once from the pack the run is actually using, so a declared
    // dominance order reaches the merge that needs it rather than being a
    // field nothing consults -- which is the shape of nine previous defects
    // in this repository.
    let precedence = ontology.precedence();

    let mut outcome =
        dre_symbolic::forward_chain(&rules, initial, &mut context, &mut trace, &precedence)?;

    classify(&pack, &mut outcome, &mut context, &mut trace)?;

    let ranking = rank_explanations(&pack, &mut outcome, &mut context, &mut trace)?;

    let plan = search_plan(&pack, &outcome.state, &mut context, &mut trace)?;

    context.note_unknown_resolution(
        started_unknown.len() as u64,
        resolved_count(&outcome.state, &started_unknown),
    );

    let proof = trace.into_graph();
    let decision_hash = decision_hash(&outcome.state, plan.as_ref());
    let record = dre_replay::ReplayRecord {
        experiment: experiment.id.clone(),
        input: input_hash,
        model: pack.model_hash(),
        engine: context.engine_hash(),
        decision: decision_hash,
        proof: proof.proof_hash(),
    };

    let failures = check(
        &experiment.expect,
        &outcome,
        &context,
        plan.as_ref(),
        &plan_writes(&pack, plan.as_ref())?,
    );

    Ok(RunResult {
        experiment: experiment.id.clone(),
        state: outcome.state,
        proof,
        fired: outcome.fired,
        counters: *context.counters(),
        record,
        plan,
        ranking,
        elapsed_micros: started.elapsed().as_micros(),
        failures,
    })
}

/// Properties the actions in a plan would write, had it been executed.
///
/// Used only to decide whether a failed expectation deserves the
/// plan/execute explanation. An empty set when there is no plan, which is
/// the common case and costs nothing.
fn plan_writes(
    pack: &dre_model::ModelPack,
    plan: Option<&dre_planner::Plan>,
) -> Result<dre_core::collections::DetSet<Id>, RunError> {
    let mut written = dre_core::collections::DetSet::new();

    let Some(plan) = plan.filter(|found| !found.actions.is_empty()) else {
        return Ok(written);
    };
    let Some(section) = parse_section::<dre_causal::CausalSection>(pack, SectionKind::Causal)?
    else {
        return Ok(written);
    };

    for chosen in &plan.actions {
        for action in section.actions.iter().filter(|action| &action.id == chosen) {
            written.extend(action.effects.keys().cloned());
        }
    }

    Ok(written)
}

/// The `DecisionHash`: everything the engine decided.
///
/// # Why the plan has to be in here
///
/// It was not, and the omission was invisible until the planner was run at
/// a scale where two problems could obviously differ. The planner works on
/// a *clone* of the state and returns a plan without applying it, so the
/// state after planning is the state before it — and hashing the state
/// alone therefore captured no part of the plan at all.
///
/// The consequence was not subtle once seen. A 5-by-5 grid solved in eight
/// moves and a 40-by-40 grid solved in seventy-eight, from different packs
/// with different obstacles, produced the **same** `DecisionHash`, because
/// both start with an agent at the origin. Two different decisions, one
/// hash.
///
/// Replay still caught a changed route, since it compares the proof too.
/// But it caught it as [`Verdict::ProofMismatch`], whose whole meaning is
/// "same decision, different reasoning" — which for a planning experiment
/// is precisely backwards. A different route is a different decision, and
/// a hash named `DecisionHash` that cannot say so is not doing its job.
///
/// [`Verdict::ProofMismatch`]: dre_replay::Verdict::ProofMismatch
#[must_use]
pub fn decision_hash_of(state: &CanonicalState, plan: Option<&dre_planner::Plan>) -> Hash32 {
    decision_hash(state, plan)
}

fn decision_hash(state: &CanonicalState, plan: Option<&dre_planner::Plan>) -> Hash32 {
    let mut writer = dre_core::canonical::CanonicalWriter::new();
    writer.tag("decision").write(state);
    // Written as an option rather than skipped, so "planned nothing" and
    // "declared no goal" stay distinguishable in the encoding.
    writer.write_option(plan);
    Hash32::of(writer.as_bytes())
}

/// Derive the pack's declared features, if it declares any.
///
/// Runs after evidence and before rules, which is the only order that
/// works: a feature reads observations, and a rule then reasons about the
/// derived quantity as though it had been measured. Reversing them would
/// make a rule's outcome depend on whether the feature it needed happened
/// to have been computed yet.
fn derive_features(
    pack: &dre_model::ModelPack,
    state: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<CanonicalState, RunError> {
    let Some(section) =
        parse_section::<dre_symbolic::FeaturesSection>(pack, SectionKind::Features)?
    else {
        return Ok(state);
    };

    Ok(dre_symbolic::derive_all(
        &section.features,
        state,
        context,
        trace,
    )?)
}

/// Rank the pack's declared hypotheses against the reasoned state.
///
/// Runs after rules and classification and before planning, which is the
/// only ordering that makes sense: a hypothesis is scored against everything
/// the engine has worked out, and the plan may then act on the diagnosis.
/// Ranking earlier would score explanations against a state the engine had
/// not finished deriving; ranking later would put the answer after the thing
/// that needed it.
///
/// # Why it may write a property
///
/// Because otherwise the layer can only produce a report, and the whole
/// point of `device-cascade` is that the *decision* was wrong. But writing
/// is opt-in: nothing happens unless the pack names `rank_into`, so a pack
/// gets a ranking it can read without one silently becoming a fact.
///
/// Two conditions guard the write, both in `Ranking::winner`: the leader
/// must have a supported mechanism, and it must **strictly** outscore the
/// runner-up. An exact tie broken by identifier order is a stable report and
/// not a conclusion, and putting it into state would make an arbitrary
/// answer indistinguishable from a real one.
fn rank_explanations(
    pack: &dre_model::ModelPack,
    outcome: &mut dre_symbolic::Outcome,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<Option<dre_explain::Ranking>, RunError> {
    let Some(section) =
        parse_section::<dre_explain::ExplanationsSection>(pack, SectionKind::Explanations)?
    else {
        return Ok(None);
    };

    let ranking = dre_explain::rank(&section, &outcome.state)?;

    // Every hypothesis is recorded, in ranked order, with the components
    // that produced its score. A proof that named only the winner would
    // answer "what did you decide" and not "why not the other one", and the
    // second question is the one a diagnosis is judged on.
    // What the ranking rests on is what the observations rest on, so the
    // score traces back through the rules and features to the measurements
    // themselves.
    //
    // Every declared observation, not only the ones a given hypothesis
    // mentions: a score depends on the whole question set, because leaving
    // something unexplained is part of it. A hypothesis that ignored an
    // observation still had that observation counted against it, and the
    // proof has to show what it was read from.
    let conditions: Vec<dre_symbolic::Condition> = section
        .observations
        .iter()
        .map(|observation| observation.when.clone())
        .collect();
    let mut read_from = dre_symbolic::referenced_subjects(&conditions);
    read_from.sort();
    read_from.dedup();

    let mut considered = Vec::new();
    for scored in &ranking.scored {
        let supported = scored
            .mechanisms
            .iter()
            .filter(|fit| fit.support == dre_explain::Support::Supported)
            .count();

        considered.push(trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::Explanation {
                hypothesis: scored.hypothesis.clone(),
                score: scored.score,
                coverage: scored.coverage,
                supported: supported as u32,
                contradicted: scored.contradicted.len() as u32,
                unexplained: scored.unexplained.len() as u32,
                assumptions: scored.assumptions as u32,
            },
            inputs: trace.sources_of(read_from.iter()),
        }));
        context.charge_proof_node();
        context.charge_reasoning_step();
    }

    let Some(winner) = ranking.winner() else {
        // Saying nothing here would be the wrong answer twice over: a pack
        // that declared hypotheses and got no verdict looks exactly like a
        // pack that declared none. The reason is recorded, because "nothing
        // supports the leader" and "the top two are tied" call for
        // different next moves — the second is what `survey` can fix.
        if let Some(leader) = ranking.scored.first() {
            let reason = if leader.is_supported() {
                "the leading hypotheses score exactly equal, so the ranking \
                 separates them only by identifier"
                    .to_owned()
            } else {
                "no declared hypothesis has a mechanism the state supports".to_owned()
            };
            trace.record(dre_proof::ProofNode {
                kind: dre_proof::NodeKind::Rejected {
                    candidate: leader.hypothesis.clone(),
                    reason,
                },
                inputs: considered,
            });
            context.charge_proof_node();
        }

        // Declared UNKNOWN rather than left absent.
        //
        // "The engine weighed these accounts and could not separate them" is
        // a different statement from "this property does not exist", and it
        // is the one a reader needs -- the same distinction as a failed read
        // against no reading. Two independently authored packs asserted
        // `status: UNKNOWN` here, having correctly predicted the tie, and
        // both failed on the property being missing rather than on the
        // reasoning.
        if let Some(target) = &section.rank_into {
            if let Some((entity, property)) = target
                .as_str()
                .rsplit_once('.')
                .and_then(|(entity, property)| Id::new(entity).ok().zip(Id::new(property).ok()))
            {
                let entity = outcome.state.resolve_entity_id(entity);
                let mut builder = StateBuilder::from_state(
                    std::mem::take(&mut outcome.state),
                    context.config().limits,
                );
                builder.declare(&entity, property, EpistemicStatus::Unknown)?;
                outcome.state = builder.build();
            }
        }
        return Ok(Some(ranking));
    };

    let decision = dre_proof::ProofNode {
        kind: dre_proof::NodeKind::Decision {
            candidate: winner.hypothesis.clone(),
        },
        inputs: considered,
    };

    let Some(target) = section.rank_into.clone() else {
        trace.record(decision);
        context.charge_proof_node();
        return Ok(Some(ranking));
    };

    // Both halves are errors, and the second one used to be silence: a
    // target like `boiler-1.` splits successfully, fails to parse as an
    // identifier, and the run returned the ranking having written nothing
    // and recorded no decision node. A pack that believed it was concluding
    // a root cause concluded nothing, and said so nowhere.
    let target_parts = target
        .as_str()
        .rsplit_once('.')
        .and_then(|(entity, property)| Id::new(entity).ok().zip(Id::new(property).ok()))
        .map(|(entity, property)| (outcome.state.resolve_entity_id(entity), property));
    let Some((entity, property)) = target_parts else {
        return Err(RunError::Explain(
            dre_explain::ExplainError::MalformedTarget {
                target: target.clone(),
            },
        ));
    };

    let mut builder =
        StateBuilder::from_state(std::mem::take(&mut outcome.state), context.config().limits);
    builder.infer(&entity, property, Value::Symbol(winner.hypothesis.clone()))?;
    outcome.state = builder.build();

    trace.establish(target, decision);
    context.charge_proof_node();

    Ok(Some(ranking))
}

/// Refuse a traversal in any goal, before the run does anything.
///
/// `goal_target` reads goal conditions by matching `Condition::Compare`
/// only, so a reach-shaped goal contributes zero estimated distance. A-star
/// then expands like Dijkstra and the `ProofHash` moves -- silently, because
/// a heuristic that underestimates is still admissible and nothing
/// downstream can tell.
///
/// **This used to live inside `search_plan`, and the claim that it was
/// "refused at load" was false.** `search_plan` returns early when the pack
/// declares no `causal.yaml`, and it only ever looked at `goals.first()` --
/// so a pack with goals and no actions was never checked, and a second goal
/// was never checked at all. An assertion about a refusal that nothing
/// exercises is exactly the shape this repository keeps finding, and this
/// one was written by the same hand that wrote the warning about it.
///
/// Checked here over **every** goal, before the state is built.
fn refuse_reach_in_goals(pack: &dre_model::ModelPack) -> Result<(), RunError> {
    let Some(goals) = parse_section::<dre_planner::GoalsSection>(pack, SectionKind::Goals)? else {
        return Ok(());
    };
    for goal in &goals.goals {
        if goal
            .conditions
            .iter()
            .any(dre_symbolic::Condition::contains_reach)
        {
            return Err(RunError::Search(format!(
                "goal {} contains a `reach`, which the planner's heuristic cannot read; \
                 express the traversal as a rule and give the goal the property that \
                 rule concludes",
                goal.id
            )));
        }
    }
    Ok(())
}

/// Plan toward the pack's first goal, if it declares actions and goals.
fn search_plan(
    pack: &dre_model::ModelPack,
    state: &CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<Option<dre_planner::Plan>, RunError> {
    let Some(causal) = parse_section::<dre_causal::CausalSection>(pack, SectionKind::Causal)?
    else {
        return Ok(None);
    };
    context.check_action_count(causal.actions.len() as u64)?;

    let Some(goals) = parse_section::<dre_planner::GoalsSection>(pack, SectionKind::Goals)? else {
        return Ok(None);
    };
    let Some(goal) = goals.goals.first() else {
        return Ok(None);
    };

    let constraints =
        parse_section::<dre_planner::ConstraintsSection>(pack, SectionKind::Constraints)?
            .unwrap_or_default()
            .constraints;

    // Read from the config rather than named here, so the strategy an
    // experiment declares is the same value that reached the `EngineHash`.
    // Naming it at the call site again would let the two drift apart, and
    // the hash would then describe a search the run did not perform.
    let strategy = context.config().strategy;

    dre_planner::plan(
        strategy,
        &causal.actions,
        goal,
        &constraints,
        state.clone(),
        context,
        trace,
    )
    .map_err(|error| RunError::Search(error.to_string()))
}

/// Read an entity's properties as a point in the declared space.
///
/// Returns the point built from whatever *was* readable, and whether every
/// declared dimension made it in. Classification only proceeds when the
/// point is complete: a point built from a default would sit somewhere the
/// model never described, and the resulting nearest region would look
/// perfectly reasonable. This is the "reason with missing observations"
/// case from plan section 19, and the honest answer is to decline.
///
/// The partial point is still returned because plan section 20 asks for
/// active dimensions as a metric. Reporting zero whenever classification
/// declined would hide the difference between a space nothing was known on
/// and one that missed by a single reading.
fn read_point(
    state: &CanonicalState,
    subject: &Id,
    space: &dre_geometry::SpaceDecl,
) -> (dre_geometry::Point, bool) {
    let mut point = dre_geometry::Point::new();

    let Some(entity) = state.entities.get(subject) else {
        return (point, false);
    };

    for dimension in &space.dimensions {
        let coordinate = entity
            .properties
            .get(&dimension.id)
            .filter(|property| {
                matches!(
                    property.status,
                    EpistemicStatus::Known | EpistemicStatus::Inferred
                )
            })
            .and_then(|property| match property.value.as_ref() {
                Some(Value::Integer(value)) => dre_core::fixed::Fixed::from_int(*value).ok(),
                Some(Value::Fixed(value)) => Some(*value),
                _ => None,
            });

        if let Some(coordinate) = coordinate {
            point.insert(dimension.id.clone(), coordinate);
        }
    }

    let complete = point.len() == space.dimensions.len();
    (point, complete)
}

/// Classify every entity of the declared type into its nearest region.
///
/// The space names an entity **type**, and each instance of that type is
/// classified separately, writing its own result onto itself.
///
/// It used to name one entity, and nothing could tell the difference: both
/// geometric packs called their single entity the same thing as its type —
/// `machine` of type `machine`, `pump` of type `pump` — so a lookup by
/// identifier and a lookup by type returned the same entity either way.
/// The ambiguity survived because no pack had two of anything.
/// `fault-network` has sixty, and could not use the section at all.
///
/// Entities are taken in canonical state order rather than in the order
/// they were declared, so the classification sequence — and therefore the
/// proof — does not depend on how the experiment file was written.
fn classify(
    pack: &dre_model::ModelPack,
    outcome: &mut dre_symbolic::Outcome,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<(), RunError> {
    let Some(section) =
        parse_section::<dre_geometry::GeometrySection>(pack, SectionKind::Geometry)?
    else {
        return Ok(());
    };

    // Checked once, against the declared space rather than per subject.
    // These are ceilings on what the *pack* declares; charging them per
    // entity would make a sixty-entity domain fail a limit describing the
    // size of a space it declares exactly once.
    context.check_dimension_count(section.space.dimensions.len() as u64)?;
    context.check_region_count(section.regions.len() as u64)?;

    let subjects: Vec<Id> = outcome
        .state
        .entities
        .iter()
        .filter(|(_, entity)| entity.type_id == section.space.subject)
        .map(|(id, _)| id.clone())
        .collect();

    if subjects.is_empty() {
        // Silence here would be the wrong answer. A pack whose geometry
        // names a type nothing instantiates has a section that does
        // nothing, and the run should say so rather than look like a pack
        // that declared no geometry at all.
        trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::Rejected {
                candidate: section.space.subject.clone(),
                reason: "no entity of this type exists in the state".to_owned(),
            },
            inputs: Vec::new(),
        });
        context.charge_proof_node();
        return Ok(());
    }

    for subject in subjects {
        classify_one(&section, &subject, outcome, context, trace)?;
    }

    Ok(())
}

/// Classify one entity against the declared space.
fn classify_one(
    section: &dre_geometry::GeometrySection,
    subject: &Id,
    outcome: &mut dre_symbolic::Outcome,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<(), RunError> {
    let space = section.space.space();
    let subject = subject.clone();

    let (point, complete) = read_point(&outcome.state, &subject, &section.space);
    context.add_active_dimensions(point.len() as u64);

    // Whatever the point was built from is what any conclusion drawn here
    // rests on, so the edges are the same in both branches.
    let read_from: Vec<Id> = point
        .keys()
        .filter_map(|dimension| subject_of(&subject, dimension))
        .collect();
    let inputs = trace.sources_of(read_from.iter());

    if !complete {
        trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::Rejected {
                candidate: subject,
                reason: format!(
                    "not all geometric dimensions are known ({} of {} readable)",
                    point.len(),
                    section.space.dimensions.len()
                ),
            },
            inputs,
        });
        context.charge_proof_node();
        return Ok(());
    }

    let ranked = dre_geometry::rank_regions(&space, &section.regions, &point)
        .map_err(|error| RunError::Geometry(error.to_string()))?;

    let Some(nearest) = ranked.first() else {
        return Ok(());
    };

    // Record every candidate, not just the winner. "Why FAILURE_A and not
    // FAILURE_B" is the question a classification usually has to answer.
    let mut considered = Vec::new();
    for candidate in &ranked {
        considered.push(trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::Geometry {
                operator: candidate.region.clone(),
            },
            inputs: inputs.clone(),
        }));
        context.charge_proof_node();
    }

    if ranked.len() > 1 {
        trace.record(dre_proof::ProofNode {
            kind: dre_proof::NodeKind::TieBreak {
                winner: nearest.region.clone(),
                rule: "distance ASC, region_id ASC".to_owned(),
            },
            // Every region that was in the running, not only the winner:
            // a tie-break is a statement about the comparison.
            inputs: considered.clone(),
        });
        context.charge_proof_node();
    }

    let mut builder =
        StateBuilder::from_state(std::mem::take(&mut outcome.state), context.config().limits);
    builder.infer(
        &subject,
        section.space.classify_into.clone(),
        Value::Symbol(nearest.region.clone()),
    )?;

    // The other two scalar geometry operators, written only when the pack
    // asks for them. Both describe the *nearest* region, which is the one
    // the classification just named, so they answer questions about that
    // classification rather than introducing a second one.
    let winner = section
        .regions
        .iter()
        .find(|region| region.id == nearest.region);

    if let (Some(property), Some(region)) = (&section.space.boundary_distance_into, winner) {
        let distance = dre_geometry::boundary_distance(&space, region, &point)
            .map_err(|error| RunError::Geometry(error.to_string()))?;
        builder.infer(&subject, property.clone(), Value::Fixed(distance))?;
    }

    if let (Some(property), Some(region)) = (&section.space.inside_region_into, winner) {
        builder.infer(
            &subject,
            property.clone(),
            Value::Boolean(region.contains(&point)),
        )?;
    }

    outcome.state = builder.build();
    context.charge_reasoning_step();

    let decision = dre_proof::ProofNode {
        kind: dre_proof::NodeKind::Decision {
            candidate: nearest.region.clone(),
        },
        inputs: considered,
    };

    // The classification writes a property, so the decision node is what
    // that property now rests on.
    match subject_of(&subject, &section.space.classify_into) {
        Some(classified) => trace.establish(classified, decision),
        None => trace.record(decision),
    };
    context.charge_proof_node();

    Ok(())
}

/// Check declared expectations, collecting every failure rather than
/// stopping at the first.
///
/// A run that reports one mismatch at a time turns a single wrong result
/// into several debugging cycles.
fn check(
    expect: &Expect,
    outcome: &dre_symbolic::Outcome,
    context: &ExecutionContext,
    plan: Option<&dre_planner::Plan>,
    plan_writes: &dre_core::collections::DetSet<Id>,
) -> Vec<String> {
    let mut failures = Vec::new();

    if let Some(expected) = &expect.plan {
        match plan {
            None => failures.push("expected a plan, none was produced".to_owned()),
            Some(actual) => {
                if &actual.actions != expected {
                    let show =
                        |ids: &[Id]| ids.iter().map(Id::as_str).collect::<Vec<_>>().join(" -> ");
                    failures.push(format!(
                        "expected plan [{}], got [{}]",
                        show(expected),
                        show(&actual.actions)
                    ));
                }
            }
        }
    }

    if let Some(expected) = expect.plan_length {
        match plan {
            None => failures.push(format!(
                "expected a plan of {expected} steps, none was produced"
            )),
            Some(actual) if actual.actions.len() != expected => failures.push(format!(
                "expected a plan of {expected} steps, got {}",
                actual.actions.len()
            )),
            Some(_) => {}
        }
    }

    for expectation in &expect.properties {
        let Some((entity_name, property_name)) = expectation.subject.as_str().rsplit_once('.')
        else {
            failures.push(format!(
                "expectation subject {} is not entity.property",
                expectation.subject
            ));
            continue;
        };

        let found = outcome
            .state
            .entities
            .iter()
            .find(|(id, _)| id.as_str() == entity_name)
            .and_then(|(_, entity)| {
                entity
                    .properties
                    .iter()
                    .find(|(id, _)| id.as_str() == property_name)
                    .map(|(_, property)| property)
            });

        let Some(property) = found else {
            failures.push(format!("{} is absent from the result", expectation.subject));
            continue;
        };

        if let Some(expected) = &expectation.value {
            if property.value.as_ref() != Some(expected) {
                // The planner searches over a *clone* of the state and
                // returns a sequence without applying it, so a property an
                // action would have changed still holds its pre-plan value.
                //
                // The hint is here because two independent authors, one of
                // whom wrote this engine, made exactly this mistake on
                // their first planning experiment. Twice is a defect in how
                // the distinction is presented, not two coincidences, and
                // the moment it bites is the moment to explain it.
                // Only when an action in the plan actually writes this
                // property. The first version fired whenever a plan
                // existed at all, and promptly attributed a numeric type
                // mismatch to the plan/execute distinction — a confident
                // explanation of the wrong thing, which is worse than no
                // explanation.
                let hint = if plan_writes.contains(&expectation.subject) {
                    " (an action in the plan writes this property, and the planner does \
                     not execute the plan: it searches over a copy of the state and \
                     returns a sequence, so the value here is the pre-plan one)"
                } else {
                    ""
                };

                failures.push(format!(
                    "{}: expected value {:?}, got {:?}{hint}",
                    expectation.subject, expected, property.value
                ));
            }
        }

        if let Some(expected) = expectation.status {
            if property.status != expected {
                failures.push(format!(
                    "{}: expected status {:?}, got {:?}",
                    expectation.subject, expected, property.status
                ));
            }
        }
    }

    if let Some(expected) = expect.rules_fired {
        let actual = context.counters().rules_fired;
        if actual != expected {
            failures.push(format!("expected {expected} rules to fire, {actual} did"));
        }
    }

    if let Some(expected) = expect.states_expanded {
        let actual = context.counters().states_expanded;
        if actual != expected {
            failures.push(format!("expected {expected} states expanded, got {actual}"));
        }
    }

    failures
}

/// Locate the repository root from an experiment path.
///
/// Experiments live in `experiments/`, so the parent of that directory is
/// the root. Falls back to the current directory when the layout differs.
#[must_use]
pub fn repo_root_for(experiment_path: &Path) -> PathBuf {
    experiment_path
        .parent()
        .and_then(|parent| parent.parent())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Run an experiment and store its record as the replay baseline.
pub fn record(path: &Path, note: &str) -> Result<(RunResult, PathBuf), RunError> {
    let result = run(path)?;
    let stored = dre_replay::StoredRecord {
        record: result.record.clone(),
        engine_version: dre_core::context::ENGINE_VERSION.to_string(),
        note: note.to_owned(),
    };

    let written = dre_replay::store::save(&repo_root_for(path), &stored)
        .map_err(|error| RunError::Store(error.to_string()))?;

    Ok((result, written))
}

/// Re-run an experiment and compare it against its **stored** record.
///
/// This is the comparison that matters. Running twice and comparing the
/// two proves the engine agrees with itself right now; comparing against
/// what was committed proves it still agrees with what it decided then.
pub fn verify_against_stored(
    path: &Path,
) -> Result<(dre_replay::Verdict, StoredComparison), RunError> {
    let experiment = load_experiment(path)?;
    let root = repo_root_for(path);

    let stored = dre_replay::store::load(&root, &experiment.id)
        .map_err(|error| RunError::Store(error.to_string()))?;

    let fresh = run(path)?;
    let verdict = dre_replay::verify(&stored.record, &fresh.record);

    Ok((
        verdict,
        StoredComparison {
            recorded_under: stored.engine_version,
            note: stored.note,
            fresh: fresh.record,
        },
    ))
}

/// Context for interpreting a stored-record comparison.
#[derive(Debug)]
pub struct StoredComparison {
    /// Engine version the baseline was recorded under.
    pub recorded_under: String,
    /// The note stored alongside it.
    pub note: String,
    /// The record the fresh run produced.
    pub fresh: dre_replay::ReplayRecord,
}

/// Re-run an experiment twice and compare the runs to each other.
///
/// Weaker than [`verify_against_stored`] -- it cannot detect drift from a
/// baseline -- but it needs no stored record, so it still has a use before
/// one exists.
pub fn verify_replay(path: &Path) -> Result<(dre_replay::Verdict, Hash32), RunError> {
    let first = run(path)?;
    let second = run(path)?;
    let verdict = dre_replay::verify(&first.record, &second.record);
    Ok((verdict, first.record.decision))
}

/// Render a value for display.
#[must_use]
pub fn show_value(value: Option<&Value>) -> String {
    match value {
        None => "—".to_owned(),
        Some(Value::Boolean(inner)) => inner.to_string(),
        Some(Value::Integer(inner)) => inner.to_string(),
        Some(Value::Fixed(inner)) => inner.to_string(),
        Some(Value::Symbol(inner) | Value::Reference(inner)) => inner.to_string(),
    }
}
