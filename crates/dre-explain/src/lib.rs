//! Phase 26D — the explanation layer.
//!
//! "Which declared hypothesis best accounts for what I observe?"
//!
//! # Why this is a crate and not a planner feature
//!
//! Two questions get confused whenever they share a module:
//!
//! ```text
//! explanation   what most plausibly explains what I observe?
//! planning      given that, what action sequence reaches the goal?
//! ```
//!
//! They have different inputs, different outputs and different failure
//! modes. An explanation can be wrong while every action in the plan is
//! valid, and a plan can be wrong about a world the explanation described
//! correctly. Folding the first into the second would make "the diagnosis
//! was wrong" and "the route was wrong" the same kind of error, and there
//! would then be nowhere to record which one happened.
//!
//! So the separation is structural rather than a convention: this crate
//! does not depend on `dre-planner`, and `dre-planner` does not depend on
//! it. The runner ranks first and plans second, and the ranking is an input
//! to the planner exactly as an observation is.
//!
//! # The failure this exists to fix
//!
//! `device-cascade` is the domain that demanded it. One power rail fails
//! and four things on the device start complaining, so two rules both fire:
//! a low rail with degraded sensors concludes `POWER`, a high temperature
//! concludes `THERMAL`. Both rules are correct in isolation and a modeller
//! would write both. The engine's answer was `CONFLICTING`, and it was
//! right to say so, because nothing in a rule system can weigh one
//! conclusion against another — a rule fires or it does not.
//!
//! `CONFLICTING` is the correct output of the wrong question. "Do my rules
//! agree?" is not "which account of this device is better?", and the second
//! question needs a layer that can hold two answers at once and compare
//! them.
//!
//! # Why the score is not a count of symptoms
//!
//! The tempting scheme is that `POWER` explains five observations and
//! `THERMAL` explains one, so `POWER` wins. It gets the right answer here
//! and it is the wrong rule, because the five are not five independent
//! facts:
//!
//! ```text
//! POWER
//!   ├── undervoltage
//!   └── bus degradation
//!         ├── temp sensor error
//!         ├── pressure sensor error
//!         ├── flow sensor error
//!         └── comms error
//! ```
//!
//! Four sensors on one degraded bus are one thing happening, reported four
//! times. Counting them as four is the evidence-counting mistake the
//! evidence layer already refuses to make: `dre-evidence` resolves by
//! *independence group* rather than by vote, exactly so that three readings
//! off one bus cannot outvote one reading off another.
//!
//! This layer reuses that idea rather than inventing a second one. **A
//! mechanism is an independence group.** Observations a hypothesis hangs
//! off the same mechanism are correlated by the pack's own declaration, so
//! the second and subsequent ones corroborate at a discount; observations
//! under different mechanisms are independent and count in full.
//!
//! The consequence is that the *shape* of the explanation decides the
//! score, not the size of the symptom list — which is the property that
//! makes the ranking hard to game by adding symptoms.
//!
//! # Two levels, because two levels is what a domain has demanded
//!
//! A hypothesis has mechanisms and a mechanism has observations. There is
//! no arbitrary nesting, no `from:` pointing at a parent mechanism, and no
//! general DAG.
//!
//! That is a deliberate stop, on the rule this project runs on: no new
//! reasoning abstraction without a failing domain that requires it. A
//! deeper tree is easy to add later and impossible to remove once packs
//! declare it, and nothing so far needs one. When a domain arrives whose
//! mechanism genuinely has a sub-mechanism, the shape of the extension will
//! be decided by that domain rather than guessed at here.
//!
//! # Where the weights live
//!
//! In the engine, as constants, covered by `ENGINE_VERSION` and therefore
//! by `EngineHash`. Not in the pack.
//!
//! A pack-tunable weight would let a modeller who did not like the ranking
//! adjust it until the intended hypothesis won, and the resulting model
//! would encode the answer rather than the reasoning. Two packs would then
//! be scoring on two different scales while reporting one number. Making
//! them engine constants means changing them is an engine change, which is
//! visible in every hash and has to be argued for in one place.

use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_state::CanonicalState;
use dre_symbolic::{evaluate, Condition, Truth};
use serde::{Deserialize, Serialize};

/// What a second observation under an already-supported mechanism adds.
///
/// The corroboration discount. The first observation a mechanism accounts
/// for is worth a whole unit; each further one is worth this much, because
/// symptoms sharing a mechanism are one thing reported repeatedly.
///
/// Not zero. Four sensors failing together is stronger support for a bus
/// fault than one sensor failing alone — it is just not four times
/// stronger. Setting this to zero would say a mechanism is fully confirmed
/// by its first symptom, which no diagnostician believes.
pub const CORROBORATION: Fixed = Fixed::from_raw(250_000);

/// What a refuted prediction costs.
///
/// Deliberately larger than what an explained observation earns. A
/// hypothesis that predicts something the world denies has been tested and
/// failed, which is a stronger signal than one more thing it got right —
/// and a hypothesis able to buy its way out of a contradiction by
/// accumulating symptoms would be unfalsifiable.
pub const CONTRADICTION: Fixed = Fixed::from_raw(2_000_000);

/// What leaving an observation unaccounted for costs.
///
/// Small, because a hypothesis is not required to be a theory of
/// everything: a power fault need not explain an open door. It is not zero,
/// because an account that ignores most of what is happening is a worse
/// account than one that does not.
pub const UNEXPLAINED: Fixed = Fixed::from_raw(250_000);

/// What each declared mechanism costs.
///
/// Occam, weighted weakly on purpose. It exists to separate two hypotheses
/// that explain the same observations equally well, one of which needs more
/// machinery to do it. It is not meant to overcome a real difference in
/// coverage, and at this weight it cannot.
pub const COMPLEXITY: Fixed = Fixed::from_raw(50_000);

/// What positing an unconfirmed mechanism costs.
///
/// The expensive one, and the reason this is not simply coverage minus
/// penalties. A mechanism nothing supports is a step in the story the world
/// has not corroborated — the hypothesis needs it to connect, and nobody has
/// checked it. Hypotheses that invent connective tissue to reach the
/// observations they want should lose to ones that do not, and this is what
/// makes them.
pub const ASSUMPTION: Fixed = Fixed::from_raw(500_000);

/// What an explanation layer could not do.
#[derive(Debug, thiserror::Error)]
pub enum ExplainError {
    /// Two observations, hypotheses or mechanisms share an identifier.
    ///
    /// Refused rather than resolved by first-wins or last-wins. Either rule
    /// makes the meaning of a pack depend on the order its author happened
    /// to type things in.
    #[error("{kind} `{id}` is declared more than once")]
    Duplicate {
        /// What was duplicated.
        kind: &'static str,
        /// The repeated identifier.
        id: Id,
    },

    /// A mechanism claims to explain an observation nobody declared.
    ///
    /// A typo here would otherwise silently shrink a hypothesis's coverage,
    /// which reads as a real finding about the world rather than as a
    /// mistake in the file.
    #[error(
        "hypothesis `{hypothesis}` explains `{observation}`, which is not a declared observation"
    )]
    NoSuchObservation {
        /// The hypothesis referring to it.
        hypothesis: Id,
        /// The identifier that matched nothing.
        observation: Id,
    },

    /// One hypothesis accounts for the same observation more than once.
    ///
    /// Refused, because coverage is credited per supported mechanism and
    /// again per observation within it, so a repeat buys independent support
    /// for a fact that was only established once. Measured before the guard
    /// existed, against a rival explaining the identical evidence honestly
    /// at 0.95: three mechanisms each naming one observation scored 2.85,
    /// and one mechanism naming it four times scored 1.70.
    ///
    /// Not an exotic mistake. Two mechanisms that both produce the same
    /// symptom is **overdetermination**, a real thing a modeller would
    /// reasonably write down, and the format having no way to express it is
    /// why this is an error rather than a quiet discount. If a domain needs
    /// overdetermination it should arrive with that domain and be scored
    /// deliberately, rather than falling out of a double count.
    #[error(
        "hypothesis `{hypothesis}` explains `{observation}` more than once; \
         an observation is one fact and may be accounted for once"
    )]
    ExplainedTwice {
        /// The hypothesis that repeats it.
        hypothesis: Id,
        /// The observation accounted for more than once.
        observation: Id,
    },

    /// `rank_into` is not a usable `entity.property` subject.
    ///
    /// Its own error, because the previous behaviour was worse than a poor
    /// message: a target whose property half was empty -- `boiler-1.` --
    /// split successfully, failed to parse as an identifier, and the run
    /// returned the ranking having written nothing and recorded no decision
    /// node. A pack that believed it was concluding a root cause concluded
    /// nothing, quietly.
    #[error("`{target}` is not a usable entity.property subject for rank_into")]
    MalformedTarget {
        /// The offending subject.
        target: Id,
    },

    /// A traversal in an observation could not be evaluated.
    #[error("observation: {0}")]
    Eval(#[from] dre_symbolic::EvalError),

    /// Fixed-point arithmetic overflowed while scoring.
    #[error("scoring `{hypothesis}` overflowed")]
    Overflow {
        /// The hypothesis being scored.
        hypothesis: Id,
    },
}

/// Something the world can be asked about, declared once for every
/// hypothesis to be scored against.
///
/// Shared rather than per-hypothesis, and that is the point: two accounts
/// of the same device are comparable only if they are answering the same
/// question set. If each hypothesis brought its own observations, a
/// hypothesis could improve its score by declaring fewer things to be
/// explained, and the ranking would measure ambition rather than fit.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Unique identifier, and the final tie-break in every ordering here.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// What has to hold in the final state for this to be happening.
    ///
    /// An ordinary [`Condition`], evaluated by the ordinary evaluator, so
    /// an observation is decidable in exactly the sense a rule's premise
    /// is — including being `UNKNOWN`, which is neither happening nor not.
    pub when: Condition,

    /// Whether this observation, when it holds, is something an account
    /// ought to explain.
    ///
    /// **Default true.** Set it false for a *negative finding* — something
    /// confirmed healthy, whose diagnostic value is entirely that it
    /// **refutes**. `dns_ok` is the example: it should contradict a DNS
    /// hypothesis and it should not be creditable to whoever thinks to
    /// mention it.
    ///
    /// The defect this fixes was found by the fifth independently authored
    /// pack, in the one experiment where its author's prediction and the
    /// engine disagreed. The author was right. A mechanism called
    /// `dependencies_clear` earned 1.75 of coverage for accounting for
    /// `dns_ok`, `network_ok` and `tls_ok`, while the rival account was
    /// charged 1.00 for leaving those same three "unexplained" — so an
    /// account was **rewarded for explaining non-problems and punished for
    /// not explaining why the working parts work**.
    ///
    /// The observation set was conflating two different things. A symptom
    /// wants accounting for; a negative finding does not. With this false,
    /// the observation earns no coverage and incurs no unexplained penalty,
    /// and contradiction is untouched — a hypothesis that predicted the
    /// opposite is still refuted by it.
    ///
    /// Very likely the same root as the networking author's separate
    /// complaint that scoring "cannot encode specificity, so a generic
    /// routing-blackhole account can tie a more specific STP-block account":
    /// coverage rewarding breadth over relevance.
    #[serde(default = "yes")]
    pub needs_explaining: bool,
}

/// Default for [`Observation::needs_explaining`].
///
/// True, so every pack written before this reads exactly as it did: an
/// observation is something to be explained unless the author says
/// otherwise.
const fn yes() -> bool {
    true
}

/// One causal step within a hypothesis, and the observations it accounts
/// for.
///
/// Also the independence group: the observations listed here are correlated
/// by declaration, so they corroborate each other at a discount rather than
/// counting separately.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mechanism {
    /// Unique within its hypothesis.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// Declared observation identifiers this mechanism would produce.
    ///
    /// A mechanism explaining nothing is legal and expensive: it is an
    /// intermediate step the hypothesis needs and nothing confirms, which
    /// is what [`ASSUMPTION`] charges for.
    #[serde(default)]
    pub explains: Vec<Id>,
}

/// A candidate account of the whole observed state.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hypothesis {
    /// Unique identifier. Written into `rank_into` when this one wins, so
    /// it is normally a symbol the ontology admits for that property.
    pub id: Id,
    /// Human-facing note.
    #[serde(default)]
    pub description: String,
    /// The causal steps this account requires.
    #[serde(default)]
    pub mechanisms: Vec<Mechanism>,
}

/// The `explanations.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplanationsSection {
    /// The shared question set.
    #[serde(default)]
    pub observations: Vec<Observation>,
    /// The competing accounts.
    #[serde(default)]
    pub hypotheses: Vec<Hypothesis>,
    /// Where to write the winner, as `entity.property`.
    ///
    /// Optional, and absent means the ranking is computed, proved and
    /// reported without touching the state. That is the honest default: a
    /// ranking is a reading of the evidence, and a pack should have to say
    /// explicitly that it wants that reading treated as a fact the planner
    /// may act on.
    #[serde(default)]
    pub rank_into: Option<Id>,
}

impl ExplanationsSection {
    /// Check the section refers only to things it declares.
    ///
    /// # Errors
    ///
    /// Fails on a duplicate identifier or a reference to an observation
    /// that is not declared.
    pub fn validate(&self) -> Result<(), ExplainError> {
        let mut seen: Vec<&Id> = Vec::new();
        for observation in &self.observations {
            if seen.contains(&&observation.id) {
                return Err(ExplainError::Duplicate {
                    kind: "observation",
                    id: observation.id.clone(),
                });
            }
            seen.push(&observation.id);
        }

        let mut hypotheses: Vec<&Id> = Vec::new();
        for hypothesis in &self.hypotheses {
            if hypotheses.contains(&&hypothesis.id) {
                return Err(ExplainError::Duplicate {
                    kind: "hypothesis",
                    id: hypothesis.id.clone(),
                });
            }
            hypotheses.push(&hypothesis.id);

            let mut mechanisms: Vec<&Id> = Vec::new();
            // Accumulated across the whole hypothesis rather than per
            // mechanism, because the expensive version of this mistake is
            // one observation hung off several mechanisms, each of which
            // then earns a full unit of coverage for the same fact.
            let mut accounted: Vec<&Id> = Vec::new();

            for mechanism in &hypothesis.mechanisms {
                if mechanisms.contains(&&mechanism.id) {
                    return Err(ExplainError::Duplicate {
                        kind: "mechanism",
                        id: mechanism.id.clone(),
                    });
                }
                mechanisms.push(&mechanism.id);

                for observation in &mechanism.explains {
                    if !seen.contains(&observation) {
                        return Err(ExplainError::NoSuchObservation {
                            hypothesis: hypothesis.id.clone(),
                            observation: observation.clone(),
                        });
                    }
                    if accounted.contains(&observation) {
                        return Err(ExplainError::ExplainedTwice {
                            hypothesis: hypothesis.id.clone(),
                            observation: observation.clone(),
                        });
                    }
                    accounted.push(observation);
                }
            }
        }

        // Checked here as well as where it is written, so `dre validate`
        // reports it rather than leaving it to a run that concludes nothing
        // and says nothing about why.
        if let Some(target) = &self.rank_into {
            let usable = target
                .as_str()
                .rsplit_once('.')
                .is_some_and(|(entity, property)| {
                    Id::new(entity).is_ok() && Id::new(property).is_ok()
                });
            if !usable {
                return Err(ExplainError::MalformedTarget {
                    target: target.clone(),
                });
            }
        }

        Ok(())
    }
}

/// Whether the world backs a mechanism, denies it, or has not said.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Support {
    /// At least one observation this mechanism explains is happening.
    Supported,
    /// Observations it predicted are not happening, and none is.
    Contradicted,
    /// Nothing it predicted is decided either way — or it predicts
    /// nothing at all.
    ///
    /// The unsupported intermediate assumption: a step the hypothesis needs
    /// that no reading confirms.
    Unsupported,
}

/// How one mechanism fared against the state.
#[derive(Clone, Debug)]
pub struct MechanismFit {
    /// The mechanism's identifier.
    pub id: Id,
    /// Explained observations that are happening.
    pub supported: Vec<Id>,
    /// Explained observations that are definitely not happening.
    pub contradicted: Vec<Id>,
    /// Explained observations nothing has decided.
    pub silent: Vec<Id>,
    /// The verdict the three lists add up to.
    pub support: Support,
}

/// One hypothesis, scored.
#[derive(Clone, Debug)]
pub struct Scored {
    /// The hypothesis's identifier.
    pub hypothesis: Id,
    /// Per-mechanism detail, in declared order.
    pub mechanisms: Vec<MechanismFit>,
    /// Explanatory credit earned, corroboration already discounted.
    pub coverage: Fixed,
    /// Observations this hypothesis predicted and the world denied.
    pub contradicted: Vec<Id>,
    /// Observations that are happening and this hypothesis does not
    /// account for.
    pub unexplained: Vec<Id>,
    /// Mechanisms declared.
    pub complexity: usize,
    /// Mechanisms nothing supports.
    pub assumptions: usize,
    /// The single number the ranking is on.
    pub score: Fixed,
}

impl Scored {
    /// Whether anything at all backs this account.
    ///
    /// The bar for writing a conclusion into state. A hypothesis with no
    /// supported mechanism can still outscore its rivals — by being simple
    /// and predicting nothing that failed — and "the least bad of several
    /// unsupported stories" is not a diagnosis.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.mechanisms
            .iter()
            .any(|fit| fit.support == Support::Supported)
    }
}

/// The result of ranking every declared hypothesis.
#[derive(Clone, Debug)]
pub struct Ranking {
    /// Best first, under a total order.
    pub scored: Vec<Scored>,
    /// Declared observations that are happening.
    pub holding: Vec<Id>,
    /// Declared observations nothing decided, reported so that a ranking
    /// made on partial information says so.
    pub undecided: Vec<Id>,
}

impl Ranking {
    /// The winner, if the ranking separated one.
    ///
    /// `None` when there are no hypotheses, when the leader has no
    /// supported mechanism, or when the top two score **exactly** equal.
    ///
    /// The last case is the interesting one, and it is why this is not just
    /// `scored.first()`. The ordering below is total, so there is always a
    /// first element — but a lead that exists only because one identifier
    /// sorts before another is a tie-break, not a finding, and reporting it
    /// as a conclusion would put an arbitrary answer into state wearing the
    /// same clothes as a real one. A ranking that cannot separate its top
    /// two has produced a genuine result: *this evidence does not decide
    /// between these accounts*, which is the question `survey` then answers
    /// by finding an observation that would.
    #[must_use]
    pub fn winner(&self) -> Option<&Scored> {
        let leader = self.scored.first()?;
        if !leader.is_supported() {
            return None;
        }
        match self.scored.get(1) {
            Some(runner_up) if runner_up.score == leader.score => None,
            _ => Some(leader),
        }
    }

    /// How far ahead one named hypothesis is, which may be behind.
    ///
    /// Its score less the best of the others, so the result is negative when
    /// the named hypothesis is not winning. That sign is the whole point: it
    /// lets a caller hold one account fixed and watch its standing move as
    /// the evidence changes, which [`Ranking::margin`] cannot do because it
    /// always reports the current leader and so silently changes subject the
    /// moment the order flips.
    ///
    /// This is what makes discriminating observations findable. An unknown
    /// whose admissible answers swing this from `+1.00` to `-1.50` decides
    /// the diagnosis; one that leaves it at `+0.50` either way is not worth
    /// the trip, however much else it might tell you.
    ///
    /// `None` if the hypothesis was not ranked. `Some` with a single
    /// hypothesis declared, which then leads by its own score.
    #[must_use]
    pub fn lead_of(&self, hypothesis: &Id) -> Option<Fixed> {
        let position = self
            .scored
            .iter()
            .position(|scored| &scored.hypothesis == hypothesis)?;
        let mine = self.scored.get(position)?.score;
        let best_other = self
            .scored
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != position)
            .map(|(_, scored)| scored.score)
            .max();
        match best_other {
            Some(other) => mine.checked_sub(other).ok(),
            None => Some(mine),
        }
    }

    /// How far ahead the leader is.
    ///
    /// The separation between the top two, or `None` with fewer than two
    /// hypotheses. This is what makes a ranking actionable or not: two
    /// accounts a hair apart are a reason to go and measure something, and
    /// the survey reads this number to decide what.
    #[must_use]
    pub fn margin(&self) -> Option<Fixed> {
        let leader = self.scored.first()?;
        let runner_up = self.scored.get(1)?;
        leader.score.checked_sub(runner_up.score).ok()
    }
}

/// Count as a fixed-point quantity.
fn count(value: usize) -> Option<Fixed> {
    i64::try_from(value)
        .ok()
        .and_then(|as_int| Fixed::from_int(as_int).ok())
}

/// Score one hypothesis against the observations that were evaluated.
fn score_one(
    hypothesis: &Hypothesis,
    holding: &[Id],
    truth: &[(Id, Truth)],
    needs_explaining: &[Id],
) -> Result<Scored, ExplainError> {
    let overflow = || ExplainError::Overflow {
        hypothesis: hypothesis.id.clone(),
    };

    let read = |observation: &Id| -> Truth {
        truth
            .iter()
            .find(|(id, _)| id == observation)
            .map_or(Truth::Unknown, |(_, value)| *value)
    };

    let mut mechanisms = Vec::new();
    let mut coverage = Fixed::ZERO;
    let mut contradicted_all: Vec<Id> = Vec::new();
    let mut explained: Vec<Id> = Vec::new();
    let mut assumptions = 0usize;

    for mechanism in &hypothesis.mechanisms {
        let mut supported = Vec::new();
        let mut contradicted = Vec::new();
        let mut silent = Vec::new();

        for observation in &mechanism.explains {
            match read(observation) {
                Truth::True => supported.push(observation.clone()),
                Truth::False => contradicted.push(observation.clone()),
                Truth::Unknown => silent.push(observation.clone()),
            }
        }

        // Credit is earned only for accounting for something that wanted
        // accounting for. A negative finding still counts toward the
        // mechanism being SUPPORTED -- it is evidence the mechanism is
        // live -- but it earns no coverage, because explaining that a
        // thing is fine is not an explanatory achievement.
        let creditable = supported
            .iter()
            .filter(|observation| needs_explaining.contains(observation))
            .count();

        // A mechanism with any live symptom is standing, whatever else it
        // also predicted. Contradiction is charged per observation below,
        // so a mechanism that got one thing right and one thing wrong pays
        // for the miss without losing credit for the hit.
        let support = if !supported.is_empty() {
            Support::Supported
        } else if !contradicted.is_empty() {
            Support::Contradicted
        } else {
            Support::Unsupported
        };

        if support == Support::Unsupported {
            assumptions = assumptions.saturating_add(1);
        }

        // Here is the whole anti-symptom-counting rule, in three lines. The
        // mechanism earns one unit for existing at all in a supported
        // state; its second and later observations earn the discount. Four
        // correlated sensor errors are therefore worth 1.75, not 4.
        if creditable > 0 {
            let extra = count(creditable.saturating_sub(1)).ok_or_else(overflow)?;
            let corroboration = extra.checked_mul(CORROBORATION).map_err(|_| overflow())?;
            coverage = coverage
                .checked_add(Fixed::ONE)
                .and_then(|running| running.checked_add(corroboration))
                .map_err(|_| overflow())?;
        }

        explained.extend(supported.iter().cloned());
        contradicted_all.extend(contradicted.iter().cloned());

        mechanisms.push(MechanismFit {
            id: mechanism.id.clone(),
            supported,
            contradicted,
            silent,
            support,
        });
    }

    // Only observations that are *happening* can be left unexplained.
    // Nobody owes an account of something the world is not doing, and
    // charging for it would make every hypothesis pay for the size of the
    // question set rather than for its own fit.
    let unexplained: Vec<Id> = holding
        .iter()
        .filter(|observation| needs_explaining.contains(observation))
        .filter(|observation| !explained.contains(observation))
        .cloned()
        .collect();

    // Raw counts here, deliberately, where coverage was discounted. The
    // discount says redundant confirmation is worth less; it does not say
    // an observation nobody addressed is a smaller failing because
    // something else was also unaddressed. Different questions, different
    // arithmetic.
    let complexity = hypothesis.mechanisms.len();
    let penalty = |weight: Fixed, quantity: usize| -> Result<Fixed, ExplainError> {
        count(quantity)
            .ok_or_else(overflow)?
            .checked_mul(weight)
            .map_err(|_| overflow())
    };

    let charges = [
        penalty(CONTRADICTION, contradicted_all.len())?,
        penalty(UNEXPLAINED, unexplained.len())?,
        penalty(COMPLEXITY, complexity)?,
        penalty(ASSUMPTION, assumptions)?,
    ];

    let mut score = coverage;
    for charge in charges {
        score = score.checked_sub(charge).map_err(|_| overflow())?;
    }

    Ok(Scored {
        hypothesis: hypothesis.id.clone(),
        mechanisms,
        coverage,
        contradicted: contradicted_all,
        unexplained,
        complexity,
        assumptions,
        score,
    })
}

/// Rank every declared hypothesis against a reasoned state.
///
/// # Errors
///
/// Fails if the section refers to an observation it does not declare, if an
/// identifier is duplicated, or if scoring overflows.
pub fn rank(
    section: &ExplanationsSection,
    state: &CanonicalState,
) -> Result<Ranking, ExplainError> {
    section.validate()?;

    // Every observation is evaluated once, against the state, before any
    // hypothesis is scored. Evaluating per hypothesis would give the same
    // answers and would let a future change make one hypothesis see a
    // different world from another, which is the one thing a comparison
    // cannot survive.
    let mut truth: Vec<(Id, Truth)> = Vec::new();
    for observation in &section.observations {
        truth.push((observation.id.clone(), evaluate(&observation.when, state)?));
    }

    let holding: Vec<Id> = truth
        .iter()
        .filter(|(_, value)| *value == Truth::True)
        .map(|(id, _)| id.clone())
        .collect();
    let undecided: Vec<Id> = truth
        .iter()
        .filter(|(_, value)| *value == Truth::Unknown)
        .map(|(id, _)| id.clone())
        .collect();

    let needs_explaining: Vec<Id> = section
        .observations
        .iter()
        .filter(|observation| observation.needs_explaining)
        .map(|observation| observation.id.clone())
        .collect();

    let mut scored = section
        .hypotheses
        .iter()
        .map(|hypothesis| score_one(hypothesis, &holding, &truth, &needs_explaining))
        .collect::<Result<Vec<_>, _>>()?;

    // Total, and total on purpose. Score decides; then fewer contradictions,
    // because a hypothesis that has been refuted less is the better bet at
    // equal fit; then fewer mechanisms, which is Occam breaking a tie it was
    // too weakly weighted to break outright; then the identifier, which is
    // unique, so the comparison never falls through to whatever order the
    // file happened to be written in.
    //
    // The identifier tie-break makes the *report* stable. It deliberately
    // does not make a *winner* — see `Ranking::winner`.
    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.contradicted.len().cmp(&right.contradicted.len()))
            .then_with(|| left.complexity.cmp(&right.complexity))
            .then_with(|| left.hypothesis.cmp(&right.hypothesis))
    });

    Ok(Ranking {
        scored,
        holding,
        undecided,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use dre_core::config::Limits;
    use dre_state::{StateBuilder, Value};
    use dre_symbolic::Operator;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn holds(subject: &str) -> Condition {
        Condition::Compare {
            subject: id(subject),
            operator: Operator::Eq,
            operand: Value::Boolean(true),
        }
    }

    fn observation(name: &str, subject: &str) -> Observation {
        Observation {
            id: id(name),
            description: String::new(),
            when: holds(subject),
            needs_explaining: true,
        }
    }

    /// An observation confirmed HEALTHY: it refutes, and is not creditable.
    fn negative_finding(name: &str, subject: &str) -> Observation {
        Observation {
            id: id(name),
            description: String::new(),
            when: holds(subject),
            needs_explaining: false,
        }
    }

    fn mechanism(name: &str, explains: &[&str]) -> Mechanism {
        Mechanism {
            id: id(name),
            description: String::new(),
            explains: explains.iter().map(|each| id(each)).collect(),
        }
    }

    fn hypothesis(name: &str, mechanisms: Vec<Mechanism>) -> Hypothesis {
        Hypothesis {
            id: id(name),
            description: String::new(),
            mechanisms,
        }
    }

    /// A device where a rail is down and four things on one bus complain,
    /// and the temperature is also high. The `device-cascade` shape.
    fn cascade_state() -> CanonicalState {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        for property in [
            "undervolt",
            "sensor_a",
            "sensor_b",
            "sensor_c",
            "comms",
            "hot",
        ] {
            builder
                .observe(&id("dev"), id(property), Value::Boolean(true))
                .expect("observe");
        }
        builder.build()
    }

    fn cascade_section() -> ExplanationsSection {
        ExplanationsSection {
            observations: vec![
                observation("undervoltage", "dev.undervolt"),
                observation("sensor_a_error", "dev.sensor_a"),
                observation("sensor_b_error", "dev.sensor_b"),
                observation("sensor_c_error", "dev.sensor_c"),
                observation("comms_error", "dev.comms"),
                observation("overheating", "dev.hot"),
            ],
            hypotheses: vec![
                hypothesis(
                    "POWER",
                    vec![
                        mechanism("rail_collapse", &["undervoltage"]),
                        mechanism(
                            "bus_degradation",
                            &[
                                "sensor_a_error",
                                "sensor_b_error",
                                "sensor_c_error",
                                "comms_error",
                            ],
                        ),
                    ],
                ),
                hypothesis(
                    "THERMAL",
                    vec![
                        mechanism("overheat", &["overheating"]),
                        mechanism("heat_damage", &["sensor_a_error"]),
                    ],
                ),
            ],
            rank_into: None,
        }
    }

    /// The defect the fifth independently authored pack found, in the one
    /// experiment where its author's prediction beat the engine.
    ///
    /// An account that mentions three healthy findings must not outscore one
    /// that explains the actual fault. Before the fix, a mechanism whose
    /// observations were `a_ok`/`b_ok`/`c_ok` earned 1.75 of coverage while
    /// its rival was charged 0.75 for leaving those same three
    /// "unexplained".
    #[test]
    fn explaining_that_things_are_fine_earns_nothing() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        for property in ["a_ok", "b_ok", "c_ok", "real_fault"] {
            builder
                .observe(&id("dev"), id(property), Value::Boolean(true))
                .expect("observe");
        }
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![
                negative_finding("a_ok", "dev.a_ok"),
                negative_finding("b_ok", "dev.b_ok"),
                negative_finding("c_ok", "dev.c_ok"),
                observation("real_fault", "dev.real_fault"),
            ],
            hypotheses: vec![
                hypothesis(
                    "BREADTH",
                    vec![mechanism(
                        "everything_else_is_fine",
                        &["a_ok", "b_ok", "c_ok"],
                    )],
                ),
                hypothesis(
                    "RELEVANT",
                    vec![mechanism("the_actual_fault", &["real_fault"])],
                ),
            ],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        assert_eq!(ranking.scored[0].hypothesis.as_str(), "RELEVANT");

        // Breadth earns nothing at all: three healthy findings, zero
        // coverage, and no penalty for the rival ignoring them either.
        let breadth = &ranking.scored[1];
        assert_eq!(breadth.coverage, Fixed::ZERO);
        assert_eq!(breadth.unexplained, vec![id("real_fault")]);

        let relevant = &ranking.scored[0];
        assert_eq!(relevant.coverage, Fixed::ONE);
        assert!(relevant.unexplained.is_empty());
    }

    /// A negative finding still REFUTES. That is its whole diagnostic value
    /// and the reason it is declared rather than omitted.
    #[test]
    fn a_negative_finding_still_contradicts() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        builder
            .observe(&id("dev"), id("dns_broken"), Value::Boolean(false))
            .expect("observe");
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![negative_finding("dns_broken", "dev.dns_broken")],
            hypotheses: vec![hypothesis(
                "DNS",
                vec![mechanism("lookup", &["dns_broken"])],
            )],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        assert_eq!(ranking.scored[0].contradicted, vec![id("dns_broken")]);
    }

    #[test]
    fn correlated_symptoms_do_not_each_count() {
        let ranking = rank(&cascade_section(), &cascade_state()).expect("ranks");
        let power = &ranking.scored[0];

        assert_eq!(power.hypothesis.as_str(), "POWER");
        // One mechanism explaining four correlated observations is worth
        // 1 + 3 x 0.25, not 4. With the single-observation rail mechanism
        // that is 2.75 of coverage rather than the 5.0 a symptom count
        // would produce.
        assert_eq!(power.coverage, Fixed::from_raw(2_750_000));
    }

    #[test]
    fn the_hypothesis_accounting_for_more_of_the_state_wins() {
        let ranking = rank(&cascade_section(), &cascade_state()).expect("ranks");

        assert_eq!(ranking.scored[0].hypothesis.as_str(), "POWER");
        assert_eq!(ranking.scored[1].hypothesis.as_str(), "THERMAL");
        assert_eq!(
            ranking.winner().map(|found| found.hypothesis.as_str()),
            Some("POWER")
        );
        // And it wins on more than a tie-break.
        assert!(ranking.margin().expect("two hypotheses") > Fixed::ZERO);
    }

    /// Growing the shared observation set moves the ranking, and this is
    /// the lever worth knowing about.
    ///
    /// Written to assert that six extra correlated symptoms *could not*
    /// overturn a lead of 1.50, on the reasoning that they corroborate at a
    /// quarter each. They overturn it comfortably, and the arithmetic says
    /// why: a correlated symptom is worth 0.25 of coverage to the
    /// hypothesis that explains it **and** relieves 0.25 of unexplained
    /// penalty, while costing every rival the full 0.25. Six of them swing
    /// 4.50 across a 1.50 lead.
    ///
    /// The discount is doing its job — 0.5 against 1.20 for an independent
    /// symptom, so correlated evidence is worth about 40% of independent
    /// evidence. What it does not do is make the ranking indifferent to how
    /// many observations a domain declares, because **the observation set
    /// is shared**: adding one helps whoever explains it and charges
    /// everybody else in full.
    ///
    /// That asymmetry has a defence and the defence is only half sound.
    /// Correlation is declared *inside* a hypothesis, so POWER has no view
    /// on whether `heat_1..6` are one thing or six, and an account offering
    /// no grouping for six live facts pays for six. What that argument does
    /// not survive is the observation that **the format chose to hide
    /// correlation inside hypotheses in the first place**, so the asymmetry
    /// is a consequence of a design decision rather than a fact about the
    /// world. It is also a way to tilt a ranking by writing the pack a
    /// particular way.
    ///
    /// **The fix is not the one first recorded here.** That was to pool the
    /// declared correlations across all hypotheses by union, and an
    /// independent review pointed out it is gameable in a fresh direction: a
    /// hypothesis could group unrelated observations together purely to cut
    /// every rival's unexplained penalty. Recorded as a rejected alternative
    /// rather than deleted, because the reason it fails is the useful part.
    ///
    /// The shape that does work is to make correlation groups **first-class
    /// in the shared observation set**, with mechanisms referring to
    /// observations or to groups. Shared questions should have shared
    /// independence structure. Not built: no domain has needed it, and the
    /// case below was constructed by hand to break the rule rather than
    /// found while modelling anything.
    #[test]
    fn growing_the_shared_observation_set_moves_the_ranking() {
        let state = {
            let mut builder = StateBuilder::new(Limits::default());
            builder.entity(id("dev"), id("device")).expect("entity");
            for property in [
                "undervolt",
                "sensor_a",
                "sensor_b",
                "sensor_c",
                "comms",
                "hot",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
            ] {
                builder
                    .observe(&id("dev"), id(property), Value::Boolean(true))
                    .expect("observe");
            }
            builder.build()
        };

        let mut section = cascade_section();
        for extra in 1..=6 {
            section.observations.push(observation(
                &format!("heat_{extra}"),
                &format!("dev.h{extra}"),
            ));
        }
        section.hypotheses[1].mechanisms[0]
            .explains
            .extend((1..=6).map(|extra| id(&format!("heat_{extra}"))));

        let ranking = rank(&section, &state).expect("ranks");
        assert_eq!(ranking.scored[0].hypothesis.as_str(), "THERMAL");

        // Pinned rather than described, so the lever cannot quietly change
        // size. THERMAL: 3.50 coverage (2.50 from seven correlated symptoms,
        // 1.00 from the second mechanism) less 1.00 for four unexplained and
        // 0.10 for two mechanisms. POWER: 2.75 coverage less 1.75 for seven
        // unexplained and 0.10.
        assert_eq!(ranking.scored[0].score, Fixed::from_raw(2_400_000));
        assert_eq!(ranking.scored[1].score, Fixed::from_raw(900_000));
    }

    #[test]
    fn a_refuted_prediction_costs_more_than_a_confirmed_one_earns() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        builder
            .observe(&id("dev"), id("undervolt"), Value::Boolean(true))
            .expect("observe");
        builder
            .observe(&id("dev"), id("sensor_a"), Value::Boolean(false))
            .expect("observe");
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![
                observation("undervoltage", "dev.undervolt"),
                observation("sensor_a_error", "dev.sensor_a"),
            ],
            hypotheses: vec![hypothesis(
                "POWER",
                vec![mechanism(
                    "rail_collapse",
                    &["undervoltage", "sensor_a_error"],
                )],
            )],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        let power = &ranking.scored[0];
        assert_eq!(power.coverage, Fixed::ONE);
        assert_eq!(power.contradicted.len(), 1);
        // 1.00 coverage - 2.00 contradiction - 0.05 complexity.
        assert_eq!(power.score, Fixed::from_raw(-1_050_000));
    }

    #[test]
    fn an_unconfirmed_mechanism_is_an_assumption_and_is_charged_for() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        builder
            .observe(&id("dev"), id("undervolt"), Value::Boolean(true))
            .expect("observe");
        builder
            .declare(
                &id("dev"),
                id("sensor_a"),
                dre_state::EpistemicStatus::Unknown,
            )
            .expect("declare");
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![
                observation("undervoltage", "dev.undervolt"),
                observation("sensor_a_error", "dev.sensor_a"),
            ],
            hypotheses: vec![
                hypothesis("LEAN", vec![mechanism("rail", &["undervoltage"])]),
                hypothesis(
                    "ELABORATE",
                    vec![
                        mechanism("rail", &["undervoltage"]),
                        mechanism("unchecked_link", &["sensor_a_error"]),
                    ],
                ),
            ],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        assert_eq!(ranking.scored[0].hypothesis.as_str(), "LEAN");
        assert_eq!(ranking.scored[1].assumptions, 1);
        // The undecided observation is reported rather than silently
        // treated as absent: the ranking was made on partial information
        // and says so.
        assert_eq!(ranking.undecided, vec![id("sensor_a_error")]);
    }

    #[test]
    fn an_exact_tie_declares_no_winner() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        builder
            .observe(&id("dev"), id("sensor_a"), Value::Boolean(true))
            .expect("observe");
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![observation("sensor_a_error", "dev.sensor_a")],
            hypotheses: vec![
                hypothesis("ALPHA", vec![mechanism("m", &["sensor_a_error"])]),
                hypothesis("BETA", vec![mechanism("m", &["sensor_a_error"])]),
            ],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        // The order is still total, so the report is stable...
        assert_eq!(ranking.scored[0].hypothesis.as_str(), "ALPHA");
        // ...but a lead that exists only because A sorts before B is not a
        // finding, and nothing is concluded from it.
        assert!(ranking.winner().is_none());
        assert_eq!(ranking.margin(), Some(Fixed::ZERO));
    }

    #[test]
    fn a_leader_nothing_supports_is_not_a_diagnosis() {
        let mut builder = StateBuilder::new(Limits::default());
        builder.entity(id("dev"), id("device")).expect("entity");
        builder
            .declare(
                &id("dev"),
                id("sensor_a"),
                dre_state::EpistemicStatus::Unknown,
            )
            .expect("declare");
        let state = builder.build();

        let section = ExplanationsSection {
            observations: vec![observation("sensor_a_error", "dev.sensor_a")],
            hypotheses: vec![hypothesis(
                "ONLY",
                vec![mechanism("m", &["sensor_a_error"])],
            )],
            rank_into: None,
        };

        let ranking = rank(&section, &state).expect("ranks");
        assert_eq!(ranking.scored.len(), 1);
        assert!(ranking.winner().is_none());
    }

    /// The bug an independent review found, and the reason the "not a symptom
    /// count" claim needed defending twice.
    ///
    /// Coverage is credited once per supported mechanism and again per
    /// observation within it, so repeating one observation buys independent
    /// support for a fact established once. Measured before the fix, against
    /// a rival explaining the identical evidence honestly at 0.95: three
    /// mechanisms each naming `o` scored **2.85**, and one mechanism naming
    /// `o` four times scored **1.70**.
    ///
    /// Worse than the shared-observation lever recorded above, because that
    /// one had to be constructed adversarially and this is what a modeller
    /// writes by accident the first time two mechanisms produce one symptom.
    #[test]
    fn one_observation_cannot_be_credited_twice_by_one_hypothesis() {
        let across = ExplanationsSection {
            observations: vec![observation("o", "dev.x")],
            hypotheses: vec![hypothesis(
                "DUPED",
                vec![mechanism("m1", &["o"]), mechanism("m2", &["o"])],
            )],
            rank_into: None,
        };
        assert!(matches!(
            across.validate(),
            Err(ExplainError::ExplainedTwice { .. })
        ));

        let within = ExplanationsSection {
            observations: vec![observation("o", "dev.x")],
            hypotheses: vec![hypothesis("REPEATED", vec![mechanism("m", &["o", "o"])])],
            rank_into: None,
        };
        assert!(matches!(
            within.validate(),
            Err(ExplainError::ExplainedTwice { .. })
        ));

        // The honest shape still passes, so the guard rejects repetition
        // rather than breadth.
        let honest = ExplanationsSection {
            observations: vec![observation("o", "dev.x"), observation("p", "dev.y")],
            hypotheses: vec![hypothesis(
                "HONEST",
                vec![mechanism("m1", &["o"]), mechanism("m2", &["p"])],
            )],
            rank_into: None,
        };
        assert!(honest.validate().is_ok());
    }

    #[test]
    fn an_unusable_rank_into_target_is_refused() {
        let section = ExplanationsSection {
            observations: vec![observation("o", "dev.x")],
            hypotheses: vec![hypothesis("H", vec![mechanism("m", &["o"])])],
            rank_into: Some(id("dev.")),
        };
        assert!(matches!(
            section.validate(),
            Err(ExplainError::MalformedTarget { .. })
        ));
    }

    #[test]
    fn explaining_something_nobody_declared_is_refused() {
        let section = ExplanationsSection {
            observations: vec![observation("declared", "dev.x")],
            hypotheses: vec![hypothesis("H", vec![mechanism("m", &["typo"])])],
            rank_into: None,
        };

        assert!(matches!(
            section.validate(),
            Err(ExplainError::NoSuchObservation { .. })
        ));
    }

    #[test]
    fn a_repeated_identifier_is_refused_rather_than_resolved() {
        let section = ExplanationsSection {
            observations: vec![observation("same", "dev.x"), observation("same", "dev.y")],
            hypotheses: Vec::new(),
            rank_into: None,
        };

        assert!(matches!(
            section.validate(),
            Err(ExplainError::Duplicate { .. })
        ));
    }
}
