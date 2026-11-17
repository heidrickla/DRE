//! Evidence resolution with more than a handful of sources.
//!
//! `resolution.rs` covers the behaviour case by case, with two or three
//! sources each. That is the right shape for checking the rules, and it is
//! the shape in which one defect hid for the whole MVP: `Source`
//! reliability was declared and never read, and every test happened to
//! give a source and its observations the same figure, so nothing noticed.
//!
//! What is here needs volume or a mixture to show at all — a bus with
//! twenty sensors on it, a source contradicting its own declared
//! trustworthiness, three-way splits, and stale readings mixed with fresh.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use dre_core::collections::DetMap;
use dre_core::id::Id;
use dre_evidence::{resolve_all, Evidence, EvidencePolicy, Source};
use dre_state::{EpistemicStatus, Value};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn evidence(name: &str, source: &str, value: i64, reliability: u32, at: u64) -> Evidence {
    Evidence {
        id: id(name),
        subject: id("machine"),
        predicate: id("temperature"),
        value: Value::Integer(value),
        source: id(source),
        reliability,
        observed_at: at,
    }
}

fn source(name: &str, reliability: u32, group: Option<&str>) -> (Id, Source) {
    (
        id(name),
        Source {
            id: id(name),
            reliability,
            independence_group: group.map(id),
        },
    )
}

fn sources(entries: Vec<(Id, Source)>) -> DetMap<Id, Source> {
    entries.into_iter().collect()
}

fn policy() -> EvidencePolicy {
    EvidencePolicy::default()
}

// ---------------------------------------------------------------------
// A source may not vouch for itself.
// ---------------------------------------------------------------------

#[test]
fn an_observation_cannot_claim_more_trust_than_its_source_has() {
    // The defect this closes. Only the per-item figure used to reach the
    // corroboration count, so an observation could claim any reliability
    // it liked regardless of what the pack said its source was worth --
    // a source vouching for itself, which is the one thing a reliability
    // weight exists to prevent.
    let items = vec![
        evidence("E001", "trusted", 850, 900, 0),
        evidence("E002", "junk", 850, 900, 0),
    ];

    let resolved = resolve_all(
        &items,
        &sources(vec![
            source("trusted", 900, None),
            source("junk", 100, None),
        ]),
        &policy(),
        0,
    );

    assert_eq!(
        resolved[0].corroboration, 1,
        "the disreputable source must not count toward corroboration"
    );
    assert_eq!(
        resolved[0].status,
        EpistemicStatus::Inferred,
        "one genuine backer is provisional support, not confidence"
    );
}

#[test]
fn a_source_may_still_discount_its_own_observation() {
    // The other half of taking the minimum. A reading is no more
    // trustworthy than its instrument and may be less: a partial
    // measurement, or a sensor reporting a degraded mode, is a weaker
    // claim than that sensor's usual output.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 200, 0),
    ];

    let resolved = resolve_all(
        &items,
        &sources(vec![
            source("sensor-a", 900, None),
            source("sensor-b", 900, None),
        ]),
        &policy(),
        0,
    );

    assert_eq!(
        resolved[0].corroboration, 1,
        "a discounted reading must not corroborate"
    );
}

#[test]
fn an_undeclared_source_keeps_its_own_figure() {
    // There is no ceiling to apply. Treating an undeclared source as
    // worthless would silently discard evidence rather than reporting a
    // model missing a declaration, which is `dre validate`'s job.
    let items = vec![
        evidence("E001", "declared", 850, 900, 0),
        evidence("E002", "never-declared", 850, 900, 0),
    ];

    let resolved = resolve_all(
        &items,
        &sources(vec![source("declared", 900, None)]),
        &policy(),
        0,
    );

    assert_eq!(resolved[0].corroboration, 2);
    assert_eq!(resolved[0].status, EpistemicStatus::Known);
}

// ---------------------------------------------------------------------
// Volume.
// ---------------------------------------------------------------------

#[test]
fn corroboration_does_not_creep_upward_with_volume() {
    // Twenty sensors on one bus are one opinion reported twenty times. The
    // two-sensor version of this is already covered; what this adds is
    // that the count stays at one however many share the cause.
    let items: Vec<Evidence> = (0..20)
        .map(|n| evidence(&format!("E{n:03}"), &format!("bus-sensor-{n}"), 850, 900, 0))
        .collect();

    let declared = sources(
        (0..20)
            .map(|n| source(&format!("bus-sensor-{n}"), 900, Some("bus-1")))
            .collect(),
    );

    let resolved = resolve_all(&items, &declared, &policy(), 0);

    assert_eq!(resolved[0].corroboration, 1);
    assert_eq!(resolved[0].status, EpistemicStatus::Inferred);
    assert_eq!(
        resolved[0].supporting.len(),
        20,
        "all twenty still support it -- they just do not corroborate each other"
    );
}

#[test]
fn more_evidence_can_turn_a_confident_answer_into_a_conflicting_one() {
    // Non-monotonicity, and deliberate rather than a defect. Adding two
    // genuinely independent sensors that disagree takes a KNOWN result to
    // CONFLICTING: the engine now has strong support for incompatible
    // claims and the design says it must not pick one.
    //
    // Worth pinning because it is surprising. Gathering more evidence made
    // the engine *less* decisive, and anyone who finds that startling
    // should read docs/decisions.md before changing it.
    let declared = sources(vec![
        source("probe-x", 900, None),
        source("probe-y", 900, None),
        source("probe-p", 900, None),
        source("probe-q", 900, None),
    ]);

    let confident = vec![
        evidence("E001", "probe-x", 850, 900, 0),
        evidence("E002", "probe-y", 850, 900, 0),
    ];
    assert_eq!(
        resolve_all(&confident, &declared, &policy(), 0)[0].status,
        EpistemicStatus::Known
    );

    let mut contested = confident;
    contested.push(evidence("E003", "probe-p", 400, 900, 0));
    contested.push(evidence("E004", "probe-q", 400, 900, 0));

    let resolved = resolve_all(&contested, &declared, &policy(), 0);
    assert_eq!(resolved[0].status, EpistemicStatus::Conflicting);
    assert_eq!(
        resolved[0].value, None,
        "a conflict must not resolve to a value"
    );
    assert_eq!(
        resolved[0].dissenting.len(),
        4,
        "every reading is recorded when none wins"
    );
}

#[test]
fn a_lopsided_but_genuinely_independent_split_still_conflicts() {
    // Five independent groups against two. Majority would say 850; the
    // engine says CONFLICTING. Recorded here so the behaviour is a
    // decision somebody made rather than something nobody noticed.
    let mut items: Vec<Evidence> = (0..5)
        .map(|n| evidence(&format!("E0{n:02}"), &format!("hot-{n}"), 850, 900, 0))
        .collect();
    items.extend((0..2).map(|n| evidence(&format!("E1{n:02}"), &format!("cold-{n}"), 400, 900, 0)));

    let declared = sources(
        (0..5)
            .map(|n| source(&format!("hot-{n}"), 900, None))
            .chain((0..2).map(|n| source(&format!("cold-{n}"), 900, None)))
            .collect(),
    );

    assert_eq!(
        resolve_all(&items, &declared, &policy(), 0)[0].status,
        EpistemicStatus::Conflicting
    );
}

#[test]
fn three_competing_values_resolve_to_the_one_with_independent_backing() {
    // Not just a two-way split. Two values have one backer each and the
    // third has two independent ones, so only it clears the bar.
    let items = vec![
        evidence("E001", "probe-x", 850, 900, 0),
        evidence("E002", "probe-y", 850, 900, 0),
        evidence("E003", "probe-p", 400, 900, 0),
        evidence("E004", "probe-q", 600, 900, 0),
    ];
    let declared = sources(vec![
        source("probe-x", 900, None),
        source("probe-y", 900, None),
        source("probe-p", 900, None),
        source("probe-q", 900, None),
    ]);

    let resolved = resolve_all(&items, &declared, &policy(), 0);
    assert_eq!(resolved[0].status, EpistemicStatus::Known);
    assert_eq!(resolved[0].value, Some(Value::Integer(850)));
    assert_eq!(
        resolved[0].dissenting,
        vec![id("E003"), id("E004")],
        "both losing readings are recorded, sorted"
    );
}

#[test]
fn a_stale_majority_does_not_outvote_a_fresh_minority() {
    // Freshness is applied before anything else, so three aged-out
    // readings cannot outweigh two current ones. The default window is 100
    // ticks and `now` is 1000, so anything before tick 900 has aged out.
    let items = vec![
        evidence("E001", "old-a", 400, 900, 500),
        evidence("E002", "old-b", 400, 900, 500),
        evidence("E003", "old-c", 400, 900, 500),
        evidence("E004", "fresh-x", 850, 900, 990),
        evidence("E005", "fresh-y", 850, 900, 990),
    ];
    let declared = sources(vec![
        source("old-a", 900, None),
        source("old-b", 900, None),
        source("old-c", 900, None),
        source("fresh-x", 900, None),
        source("fresh-y", 900, None),
    ]);

    let resolved = resolve_all(&items, &declared, &policy(), 1_000);
    assert_eq!(resolved[0].status, EpistemicStatus::Known);
    assert_eq!(resolved[0].value, Some(Value::Integer(850)));
    assert_eq!(
        resolved[0].dissenting,
        vec![id("E001"), id("E002"), id("E003")],
        "aged-out readings are recorded as dissent, not silently dropped"
    );
}

#[test]
fn resolution_is_independent_of_declaration_order() {
    // The ordering attack, applied to evidence rather than to rules. With
    // a dozen competing readings there is ample room for an input-order
    // dependency to hide.
    let build = |reversed: bool| {
        let mut items: Vec<Evidence> = (0..12)
            .map(|n| {
                evidence(
                    &format!("E{n:03}"),
                    &format!("probe-{n}"),
                    if n % 3 == 0 { 400 } else { 850 },
                    900,
                    0,
                )
            })
            .collect();
        if reversed {
            items.reverse();
        }
        let declared = sources(
            (0..12)
                .map(|n| source(&format!("probe-{n}"), 900, None))
                .collect(),
        );
        format!("{:?}", resolve_all(&items, &declared, &policy(), 0)[0])
    };

    assert_eq!(build(false), build(true));
}

#[test]
fn many_properties_resolve_in_property_order_not_input_order() {
    // Output ordering, which the runner relies on: resolutions are written
    // into state and recorded in the proof in the order they come back, so
    // an input-order dependency here would move the ProofHash.
    let mut items = Vec::new();
    for (index, predicate) in ["vibration", "pressure", "temperature"]
        .into_iter()
        .enumerate()
    {
        for backer in 0..2 {
            items.push(Evidence {
                id: id(&format!("E{index}{backer}")),
                subject: id("machine"),
                predicate: id(predicate),
                value: Value::Integer(100),
                source: id(&format!("probe-{index}-{backer}")),
                reliability: 900,
                observed_at: 0,
            });
        }
    }

    let declared = sources(
        (0..3)
            .flat_map(|index| (0..2).map(move |backer| (index, backer)))
            .map(|(index, backer)| source(&format!("probe-{index}-{backer}"), 900, None))
            .collect(),
    );

    let subjects: Vec<String> = resolve_all(&items, &declared, &policy(), 0)
        .iter()
        .map(|resolved| resolved.subject.to_string())
        .collect();

    assert_eq!(
        subjects,
        [
            "machine.pressure",
            "machine.temperature",
            "machine.vibration"
        ],
        "resolutions come back in property order, whatever order they arrived in"
    );
}
