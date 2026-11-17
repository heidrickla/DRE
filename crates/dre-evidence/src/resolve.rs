//! Deterministic evidence resolution.
//!
//! The state builder's merge rule already *detects* conflict: two
//! disagreeing observations give `CONFLICTING`. That is deliberately as
//! far as it goes, because resolving a disagreement needs to know where
//! each claim came from, and the builder does not carry provenance.
//!
//! This is where provenance arrives, and it is what makes the difference
//! between "two sensors disagree, give up" and "three independent
//! high-reliability sources say 850 and one stale low-reliability source
//! says 400, so the answer is 850 and here is why".
//!
//! # Why counting sources is not enough
//!
//! The naive rule — believe whichever value has the most sources — fails
//! on the case that matters. Three sensors wired to one failed bus are not
//! three opinions; they are one common cause reported three times. So
//! corroboration counts **independence groups**, not sources. A source
//! with no declared group is its own group, which makes the safe reading
//! the default: unrelated until someone says otherwise.
//!
//! # Why thresholds rather than probabilities
//!
//! Bayesian updating would need priors no model pack can honestly supply,
//! and would produce a distribution that something downstream has to
//! threshold anyway. Integer weights and declared thresholds are cruder
//! but auditable: the proof can state exactly which sources agreed and
//! which rule promoted the result.

use dre_core::collections::{DetMap, DetSet};
use dre_core::id::Id;
use dre_state::{EpistemicStatus, Value};

use crate::{Evidence, EvidencePolicy, Source};

/// Outcome of resolving competing evidence for one property.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    /// The property, as `entity.property`.
    pub subject: Id,
    /// Resolved value, absent when unresolved or conflicting.
    pub value: Option<Value>,
    /// Resulting epistemic status.
    pub status: EpistemicStatus,
    /// Evidence that supported the outcome, by identifier.
    pub supporting: Vec<Id>,
    /// Evidence that contradicted it. Retained rather than discarded, so
    /// the proof can show what was overruled.
    pub dissenting: Vec<Id>,
    /// Independent high-reliability groups backing the winning value.
    pub corroboration: u32,
}

/// How one candidate value fared.
struct Candidate {
    value: Value,
    supporting: Vec<Id>,
    independent_high: u32,
}

/// Resolve every property that has evidence.
///
/// `now` is a logical tick supplied with the input, never a clock reading —
/// the same experiment run today and next year must reach the same
/// conclusion about staleness.
#[must_use]
pub fn resolve_all(
    items: &[Evidence],
    sources: &DetMap<Id, Source>,
    policy: &EvidencePolicy,
    now: u64,
) -> Vec<Resolved> {
    // Group by subject.predicate in a deterministic map, so the output
    // order is the property order rather than the input order.
    let mut grouped: DetMap<Id, Vec<&Evidence>> = DetMap::new();

    for item in items {
        let subject = match Id::new(format!("{}.{}", item.subject, item.predicate)) {
            Ok(subject) => subject,
            // A malformed pairing cannot name a property, so there is
            // nothing to resolve it onto.
            Err(_) => continue,
        };
        grouped.entry(subject).or_default().push(item);
    }

    grouped
        .into_iter()
        .map(|(subject, mut evidence)| {
            // Sort by evidence id so ties inside one property resolve the
            // same way regardless of declaration order.
            evidence.sort_by(|left, right| left.id.cmp(&right.id));
            resolve_one(subject, &evidence, sources, policy, now)
        })
        .collect()
}

fn resolve_one(
    subject: Id,
    evidence: &[&Evidence],
    sources: &DetMap<Id, Source>,
    policy: &EvidencePolicy,
    now: u64,
) -> Resolved {
    let (fresh, stale): (Vec<&&Evidence>, Vec<&&Evidence>) = evidence
        .iter()
        .partition(|item| now.saturating_sub(item.observed_at) <= policy.freshness_window);

    if fresh.is_empty() {
        // Everything known about this property has aged out. That is not
        // the same as never having been observed, and the status says so.
        return Resolved {
            subject,
            value: None,
            status: EpistemicStatus::Stale,
            supporting: Vec::new(),
            dissenting: stale.iter().map(|item| item.id.clone()).collect(),
            corroboration: 0,
        };
    }

    // Group the fresh evidence by claimed value.
    let mut by_value: DetMap<Value, Vec<&Evidence>> = DetMap::new();
    for item in &fresh {
        by_value.entry(item.value.clone()).or_default().push(item);
    }

    let candidates: Vec<Candidate> = by_value
        .into_iter()
        .map(|(value, backers)| Candidate {
            value,
            independent_high: independent_high_count(&backers, sources, policy),
            supporting: backers.iter().map(|item| item.id.clone()).collect(),
        })
        .collect();

    let corroborated: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.independent_high >= policy.corroboration_required)
        .collect();

    let stale_ids = || stale.iter().map(|item| item.id.clone()).collect::<Vec<_>>();

    let dissent_against = |winner: &Candidate| {
        let mut out: Vec<Id> = candidates
            .iter()
            .filter(|other| other.value != winner.value)
            .flat_map(|other| other.supporting.iter().cloned())
            .collect();
        out.extend(stale_ids());
        out.sort();
        out
    };

    // Slice patterns rather than length checks plus indexing: the pattern
    // *is* the guarantee, so there is nothing for the panic lint to object
    // to and nothing to keep in sync if the arms are reordered.
    match corroborated.as_slice() {
        // Exactly one value clears the bar: the engine is confident.
        [winner] => Resolved {
            subject,
            value: Some(winner.value.clone()),
            status: EpistemicStatus::Known,
            supporting: winner.supporting.clone(),
            dissenting: dissent_against(winner),
            corroboration: winner.independent_high,
        },

        // Two or more values are each independently corroborated. This is
        // the honest failure: the engine has strong support for
        // incompatible claims and must not pick one.
        [_, _, ..] => Resolved {
            subject,
            value: None,
            status: EpistemicStatus::Conflicting,
            supporting: Vec::new(),
            dissenting: {
                let mut all: Vec<Id> = candidates
                    .iter()
                    .flat_map(|candidate| candidate.supporting.iter().cloned())
                    .collect();
                all.extend(stale_ids());
                all.sort();
                all
            },
            corroboration: 0,
        },

        // Nothing clears the bar.
        [] => {
            if let [only] = candidates.as_slice() {
                // One value, just not enough independent backing for
                // KNOWN. Believing it provisionally is what INFERRED is
                // for -- it records that the engine is going on weak
                // support, and downstream can see that.
                Resolved {
                    subject,
                    value: Some(only.value.clone()),
                    status: EpistemicStatus::Inferred,
                    supporting: only.supporting.clone(),
                    dissenting: stale_ids(),
                    corroboration: only.independent_high,
                }
            } else {
                Resolved {
                    subject,
                    value: None,
                    status: EpistemicStatus::Conflicting,
                    supporting: Vec::new(),
                    dissenting: {
                        let mut all: Vec<Id> = candidates
                            .iter()
                            .flat_map(|candidate| candidate.supporting.iter().cloned())
                            .collect();
                        all.extend(stale_ids());
                        all.sort();
                        all
                    },
                    corroboration: 0,
                }
            }
        }
    }
}

/// How much to trust one observation.
///
/// The lesser of what the source is declared to be worth and what the
/// observation claims for itself.
///
/// # Why the minimum, and why this was a defect
///
/// `Source::reliability` was declared and never read: only the per-item
/// figure reached this function. So an observation could claim any
/// reliability it liked regardless of what its source was declared to be
/// worth — a source could vouch for itself, which is the one thing a
/// reliability weight exists to prevent.
///
/// The minimum is the reading that closes it without discarding the other
/// half. A reading is no more trustworthy than the instrument that took
/// it, and may be *less*: a partial measurement or a sensor reporting a
/// degraded mode is a weaker claim than that sensor's usual output. So the
/// source sets a ceiling and the observation may lower it.
///
/// An observation from a source the pack never declared keeps its own
/// figure. There is no ceiling to apply, and treating an undeclared source
/// as worthless would silently discard evidence rather than reporting a
/// model that is missing a declaration — which is `dre validate`'s job.
fn trust(item: &Evidence, sources: &DetMap<Id, Source>) -> u32 {
    sources
        .get(&item.source)
        .map_or(item.reliability, |source| {
            source.reliability.min(item.reliability)
        })
}

/// Count distinct independence groups backing a value at high reliability.
///
/// A source with no declared group counts as its own, so sources are
/// independent by default and shared causes have to be declared. The
/// opposite default would quietly turn one bus failure into corroboration.
fn independent_high_count(
    backers: &[&Evidence],
    sources: &DetMap<Id, Source>,
    policy: &EvidencePolicy,
) -> u32 {
    let mut groups: DetSet<Id> = DetSet::new();

    for item in backers {
        if trust(item, sources) < policy.high_reliability {
            continue;
        }
        let group = sources
            .get(&item.source)
            .and_then(|source| source.independence_group.clone())
            .unwrap_or_else(|| item.source.clone());
        groups.insert(group);
    }

    u32::try_from(groups.len()).unwrap_or(u32::MAX)
}
