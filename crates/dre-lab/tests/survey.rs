//! Which observation is worth taking, and which are not worth the trip.
//!
//! A survey is counterfactuals in bulk, so it inherits their guarantee and
//! adds one risk of its own: it has to decide *which* unknowns are
//! questions. Getting that wrong does not produce a wrong answer, it
//! produces an unreadable one, and both failures showed up the first time
//! this ran.
//!
//! Ranking on the `DecisionHash` made every unknown look decisive, because
//! that hash covers the state and the state contains the observation just
//! injected. And surveying every unknown made `fault-network` report 240
//! entries, because that pack declares its conclusions UNKNOWN so the
//! resolution metric has a denominator — asking what it would conclude if
//! told its own conclusion is not a question.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_lab::runner;
use dre_lab::survey::{survey, Survey};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ask(name: &str) -> Survey {
    let path = repo_root().join("experiments").join(name);
    let experiment = runner::load_experiment(&path).expect("experiment loads");
    let pack_dir = runner::resolve_pack(&path, &experiment.pack);
    let pack = dre_model::load_dir(&pack_dir).expect("pack loads");
    survey(&experiment, &pack, &pack_dir).expect("survey runs")
}

fn subjects(result: &Survey) -> Vec<String> {
    result
        .surveyed
        .iter()
        .map(|entry| entry.subject.to_string())
        .collect()
}

#[test]
fn the_more_valuable_measurement_is_ranked_first() {
    let result = ask("haulage-001.yaml");

    // Both unknowns can change the conclusion, so the count of reachable
    // decisions ties them. What separates them is what the answer could
    // buy: rating the bridge can drop the plan from 7 to 2, rating the ford
    // can only raise it to 9. Without the cost tie-break this ordered
    // alphabetically and put the useless measurement first.
    assert_eq!(
        subjects(&result),
        vec!["north_bridge.rating_ok", "ford.rating_ok"]
    );
}

#[test]
fn a_measurement_that_can_only_help_is_distinguishable_from_one_that_cannot() {
    let result = ask("haulage-001.yaml");
    let base = result.base_cost.expect("haulage plans");

    let bridge = &result.surveyed[0];
    let ford = &result.surveyed[1];

    // Rating the bridge: best case beats the base, worst case matches it.
    assert!(bridge.best_cost().expect("a cost") < base);
    assert_eq!(bridge.worst_cost().expect("a cost"), base);

    // Rating the ford: best case only matches the base, worst case is
    // worse. Going out to measure it cannot improve the plan and might
    // reveal the ford is unusable.
    assert_eq!(ford.best_cost().expect("a cost"), base);
    assert!(ford.worst_cost().expect("a cost") > base);
}

#[test]
fn only_the_value_that_matters_is_marked_as_changing_anything() {
    let result = ask("haulage-001.yaml");
    let bridge = &result.surveyed[0];

    // `true` opens the bridge and changes the route. `false` closes it,
    // which is what an unrated bridge already was, so the engine concludes
    // exactly what it concluded before.
    //
    // On the DecisionHash both read as changes, because both write a Known
    // value into the state. That is the bug this asserts against.
    let changed: Vec<bool> = bridge
        .outcomes
        .iter()
        .map(|outcome| outcome.decision_changed)
        .collect();
    assert_eq!(changed, vec![true, false]);
}

#[test]
fn a_property_the_pack_derives_is_not_an_observation() {
    // Every unknown in `fault-network` is a conclusion: `locally_faulty`,
    // `impaired` and `is_root_cause` come from rules, `severity` from the
    // geometric space. None is something anybody could measure, so there is
    // nothing to survey — and the report says so in one line rather than in
    // 240.
    let result = ask("fault-network-001.yaml");

    assert!(
        result.surveyed.is_empty(),
        "surveyed derived properties: {:?}",
        subjects(&result)
    );
    assert!(result.declined.is_empty());
}

#[test]
fn an_unobservable_property_is_not_an_observation_either() {
    // `diagnosis-001` declares a property UNOBSERVABLE, which is a
    // permanent statement about the model rather than a gap evidence could
    // close. The epistemic status does this work with no special case here.
    let result = ask("diagnosis-001.yaml");

    assert!(result.surveyed.is_empty());
    assert!(result.declined.is_empty());
}

#[test]
fn surveying_twice_gives_the_same_report() {
    // The ranking keys are not total on their own — two properties reaching
    // the same number of decisions at the same best cost tie — so the order
    // ends on the subject name. Without that, a report about a
    // deterministic engine could come back shuffled.
    let first = ask("haulage-001.yaml");
    let second = ask("haulage-001.yaml");

    assert_eq!(subjects(&first), subjects(&second));
    assert_eq!(first.base_decision, second.base_decision);

    for (left, right) in first.surveyed.iter().zip(second.surveyed.iter()) {
        assert_eq!(left.best_cost(), right.best_cost());
        assert_eq!(left.worst_cost(), right.worst_cost());
        let left_changed: Vec<bool> = left.outcomes.iter().map(|o| o.decision_changed).collect();
        let right_changed: Vec<bool> = right.outcomes.iter().map(|o| o.decision_changed).collect();
        assert_eq!(left_changed, right_changed);
    }
}

#[test]
fn the_base_is_the_experiment_as_written() {
    // A survey must not perturb the thing it is measuring against.
    let result = ask("haulage-001.yaml");
    let path = repo_root().join("experiments").join("haulage-001.yaml");
    let plain = runner::run(&path).expect("plain run");

    assert_eq!(result.base_decision, plain.record.decision);
    assert_eq!(result.base_cost, plain.plan.map(|plan| plan.cost));
}

/// Numerics are surveyable, and the probes are the pack's own thresholds.
///
/// The survey used to decline every numeric on the grounds that sampling the
/// bounds would miss the interesting middle. True, and the conclusion did not
/// follow: the middle is exactly where the thresholds are, and the pack has
/// already written them down. An IT diagnosis whose ranking sat 0.20 apart
/// with all three settling measurements numeric is what forced it — the
/// survey reported "no single observation would change this decision" while
/// one of the three would have overturned the leader.
#[test]
fn a_numeric_is_probed_at_the_thresholds_the_pack_declares() {
    let result = ask("checkout-layer-diagnosis-001.yaml");

    let lock_wait = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.db_lock_wait_ms")
        .expect("a numeric the pack compares directly is surveyable");

    let probed: Vec<i64> = lock_wait
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome.value {
            dre_state::Value::Integer(at) => Some(at),
            _ => None,
        })
        .collect();

    // Two declared thresholds, each probed just below, at, and just above:
    // `GT` and `GTE` differ exactly there, and a metric that could not tell
    // them apart would be guessing about the boundary it was asked about.
    assert_eq!(
        probed,
        vec![19, 20, 21, 79, 80, 81],
        "probes should be the declared thresholds and their neighbours"
    );
}

/// The measurement worth taking is the one that can overturn the leader.
///
/// Not the one that confirms it. This is the whole point of separating
/// `flips_leader` from `decisive`, and the pack that motivated it was
/// authored blind by someone who predicted a different measurement.
#[test]
fn the_discriminating_measurement_can_overturn_the_leader() {
    let result = ask("checkout-layer-diagnosis-001.yaml");

    let lock_wait = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.db_lock_wait_ms")
        .expect("surveyed");
    assert!(
        lock_wait.flips_leader(),
        "db_lock_wait_ms should be able to put a different account in front"
    );

    let cpu_ready = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.hypervisor_cpu_ready_pct")
        .expect("surveyed");
    assert!(
        !cpu_ready.flips_leader(),
        "hypervisor_cpu_ready_pct separates nothing in this pack"
    );
}

/// A threshold on a derived value is probed on the measurement that feeds it.
///
/// The pack subtracts a baseline of 8 from `datastore_write_p95_ms` and
/// thresholds the difference — `GTE 25` and `LTE 10` — so the raw measurement
/// is compared against nothing directly and used to be declined. The
/// boundaries are therefore at 33 and 18, each probed with its two
/// neighbours because `GT` and `GTE` differ exactly there.
///
/// This is the measurement an outside expert, working blind, chose to take
/// next. The survey could not consider it at all.
#[test]
fn a_threshold_on_a_derived_value_is_probed_on_the_measurement_that_feeds_it() {
    let result = ask("checkout-layer-diagnosis-001.yaml");

    let entry = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.datastore_write_p95_ms")
        .expect("the measurement feeding a thresholded feature is surveyable");

    let probed: Vec<i64> = entry
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome.value {
            dre_state::Value::Integer(at) => Some(at),
            _ => None,
        })
        .collect();

    assert_eq!(
        probed,
        vec![17, 18, 19, 32, 33, 34],
        "boundaries at 18 (delta LTE 10) and 33 (delta GTE 25), each with neighbours"
    );
    // A fixed-point probe on an integer property is undecidable at run time,
    // so a carried probe must arrive in the measurement's own kind.
    assert_eq!(probed.len(), entry.outcomes.len());
}

/// The measurement an expert chose confirms; it does not overturn.
///
/// This settles a disagreement. The author of this pack predicted a *normal*
/// reading here would leave their storage hypothesis "actively refuted", and
/// chose the measurement on those grounds. It cannot: readings below the
/// boundary leave the leader exactly where it was, and readings above it only
/// widen the lead. The measurement that can overturn the diagnosis is
/// `db_lock_wait_ms`, which the author named second.
#[test]
fn the_carried_measurement_confirms_rather_than_overturns() {
    let result = ask("checkout-layer-diagnosis-001.yaml");

    let carried = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.datastore_write_p95_ms")
        .expect("surveyed");
    assert!(
        !carried.flips_leader(),
        "no reading of this measurement puts another account in front"
    );
    assert!(
        carried.discrimination() > Some(dre_core::fixed::Fixed::ZERO),
        "it still moves the lead, so it is worth taking — just not decisive"
    );

    let direct = result
        .surveyed
        .iter()
        .find(|entry| entry.subject.as_str() == "incident.db_lock_wait_ms")
        .expect("surveyed");
    assert!(
        direct.flips_leader(),
        "the author's second choice is the one that can overturn the diagnosis"
    );
}

/// Nothing is left unsurveyable on the pack that forced this.
#[test]
fn nothing_is_declined_on_the_pack_that_forced_this() {
    let result = ask("checkout-layer-diagnosis-001.yaml");
    assert!(
        result.declined.is_empty(),
        "every measurement in this pack should now be probeable, got: {:?}",
        result.declined
    );
}

/// A guarded subtraction is carried back, because the probe satisfies the
/// guard.
///
/// These features are guarded on `subject: X, operator: KNOWN`, which is
/// **false** in the base state precisely because `X` is the unknown being
/// surveyed — and true in every run that injects a probe. Seeding blind and
/// letting the run decide is the only policy that gets that right.
///
/// A second, independently authored pack, and it names its entity instance
/// `incident-001` against a type of `incident`. Matching pack subjects
/// literally worked only because the first pack happened to name its instance
/// after its type.
#[test]
fn a_guarded_subtraction_is_carried_back_on_an_independently_authored_pack() {
    let result = ask("web-outage-001-dns.yaml");

    for (property, expected) in [
        ("incident-001.db_replica_lag_seconds", vec![29, 30, 31]),
        (
            "incident-001.ad_replication_lag_seconds",
            vec![299, 300, 301],
        ),
    ] {
        let entry = result
            .surveyed
            .iter()
            .find(|entry| entry.subject.as_str() == property)
            .unwrap_or_else(|| panic!("{property} should be surveyable"));

        let probed: Vec<i64> = entry
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome.value {
                dre_state::Value::Integer(at) => Some(at),
                _ => None,
            })
            .collect();
        assert_eq!(probed, expected, "{property}");
        assert!(
            entry.flips_leader(),
            "{property} should be able to overturn the diagnosis"
        );
    }
}

/// Making the pack falsifiable changes which measurement the engine
/// recommends — and vindicates the author who wrote the original.
///
/// The two packs differ in one respect: the original declares each
/// measurement as `..._high` beside `..._normal`, two one-way booleans that
/// nothing can infer false, and the variant declares one property with both
/// polarities inferred. Everything else is identical.
///
/// The author predicted they would measure `datastore_write_p95_ms` next, on
/// the explicit grounds that a normal reading would leave their storage
/// hypothesis "actively refuted". Against the pack as written that is
/// impossible — concluding `normal` leaves `high` UNKNOWN, an unknown neither
/// supports nor contradicts, and the measurement can only ever confirm. So
/// the engine recommended `db_lock_wait_ms` instead, correctly *about that
/// pack* and wrongly about the domain.
///
/// Repair the exclusivity and the author is right: their measurement becomes
/// the most discriminating one available, and a normal reading drives the
/// storage account from first place to fourth by contradiction.
///
/// This is the test that the remedy `dre validate` recommends, and
/// `external-pack-brief.md` prescribes, actually works.
#[test]
fn repairing_exclusivity_changes_which_measurement_is_recommended() {
    let broken = ask("checkout-layer-diagnosis-001.yaml");
    let repaired = ask("checkout-layer-falsifiable-001.yaml");

    let find = |result: &Survey, name: &str| -> (bool, dre_core::fixed::Fixed) {
        let entry = result
            .surveyed
            .iter()
            .find(|entry| entry.subject.as_str() == name)
            .unwrap_or_else(|| panic!("{name} should be surveyed"));
        (
            entry.flips_leader(),
            entry
                .discrimination()
                .unwrap_or(dre_core::fixed::Fixed::ZERO),
        )
    };

    let (broken_flips, broken_spread) = find(&broken, "incident.datastore_write_p95_ms");
    assert!(
        !broken_flips,
        "as written, the author's measurement cannot overturn anything"
    );

    let (repaired_flips, repaired_spread) = find(&repaired, "incident.datastore_write_p95_ms");
    assert!(
        repaired_flips,
        "once the account can be contradicted, the author's measurement overturns it"
    );
    assert!(
        repaired_spread > broken_spread,
        "the same measurement separates the accounts further: {broken_spread} -> {repaired_spread}"
    );

    // And it overtakes the engine's original recommendation, so the repair
    // changes the advice rather than merely the arithmetic behind it.
    let (_, rival) = find(&repaired, "incident.db_lock_wait_ms");
    assert!(
        repaired_spread > rival,
        "the author's first choice should now outrank the engine's: {repaired_spread} vs {rival}"
    );
}
