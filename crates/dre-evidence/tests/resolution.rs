//! Evidence resolution behaviour.

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

fn source(name: &str, group: Option<&str>) -> (Id, Source) {
    (
        id(name),
        Source {
            id: id(name),
            reliability: 900,
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

#[test]
fn two_independent_high_reliability_sources_agreeing_gives_known() {
    // The worked example from plan section 11.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
    ];
    let sources = sources(vec![source("sensor-a", None), source("sensor-b", None)]);

    let resolved = resolve_all(&items, &sources, &policy(), 0);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].status, EpistemicStatus::Known);
    assert_eq!(resolved[0].value, Some(Value::Integer(850)));
    assert_eq!(resolved[0].corroboration, 2);
}

#[test]
fn sources_sharing_a_cause_do_not_corroborate() {
    // Three sensors on one failed bus are one opinion reported three
    // times. Counting sources rather than independence groups would read
    // this as overwhelming agreement.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
        evidence("E003", "sensor-c", 850, 900, 0),
    ];
    let sources = sources(vec![
        source("sensor-a", Some("bus-1")),
        source("sensor-b", Some("bus-1")),
        source("sensor-c", Some("bus-1")),
    ]);

    let resolved = resolve_all(&items, &sources, &policy(), 0);
    assert_eq!(resolved[0].corroboration, 1, "one bus is one group");
    assert_eq!(
        resolved[0].status,
        EpistemicStatus::Inferred,
        "believed, but not KNOWN on a single independent group"
    );
}

#[test]
fn undeclared_sources_are_independent_by_default() {
    // The safe default: unrelated until someone declares otherwise. The
    // opposite would quietly turn a common cause into corroboration.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
    ];
    let sources = sources(vec![source("sensor-a", None), source("sensor-b", None)]);
    assert_eq!(
        resolve_all(&items, &sources, &policy(), 0)[0].corroboration,
        2
    );
}

#[test]
fn low_reliability_agreement_is_inferred_not_known() {
    let items = vec![
        evidence("E001", "sensor-a", 850, 100, 0),
        evidence("E002", "sensor-b", 850, 100, 0),
    ];
    let sources = sources(vec![source("sensor-a", None), source("sensor-b", None)]);

    let resolved = resolve_all(&items, &sources, &policy(), 0);
    assert_eq!(resolved[0].status, EpistemicStatus::Inferred);
    assert_eq!(resolved[0].value, Some(Value::Integer(850)));
}

#[test]
fn a_reliable_majority_overrules_a_lone_dissenter() {
    // The case the state builder's merge rule cannot handle: it would see
    // two disagreeing observations and return CONFLICTING. With
    // provenance, the answer is 850, and the dissent is recorded rather
    // than discarded.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
        evidence("E003", "sensor-c", 400, 200, 0),
    ];
    let sources = sources(vec![
        source("sensor-a", None),
        source("sensor-b", None),
        source("sensor-c", None),
    ]);

    let resolved = resolve_all(&items, &sources, &policy(), 0);
    assert_eq!(resolved[0].status, EpistemicStatus::Known);
    assert_eq!(resolved[0].value, Some(Value::Integer(850)));
    assert_eq!(
        resolved[0].dissenting,
        vec![id("E003")],
        "the overruled claim must still be recorded"
    );
}

#[test]
fn two_corroborated_but_incompatible_values_conflict() {
    // Strong support for incompatible claims. The engine must not pick.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
        evidence("E003", "sensor-c", 400, 900, 0),
        evidence("E004", "sensor-d", 400, 900, 0),
    ];
    let sources = sources(vec![
        source("sensor-a", None),
        source("sensor-b", None),
        source("sensor-c", None),
        source("sensor-d", None),
    ]);

    let resolved = resolve_all(&items, &sources, &policy(), 0);
    assert_eq!(resolved[0].status, EpistemicStatus::Conflicting);
    assert_eq!(resolved[0].value, None, "a conflict must not pick a winner");
}

#[test]
fn evidence_past_the_freshness_window_is_stale_not_absent() {
    let items = vec![evidence("E001", "sensor-a", 850, 900, 0)];
    let sources = sources(vec![source("sensor-a", None)]);

    // Default window is 100 ticks; observe at 0, ask at 500.
    let resolved = resolve_all(&items, &sources, &policy(), 500);
    assert_eq!(resolved[0].status, EpistemicStatus::Stale);
    assert_eq!(resolved[0].value, None);
    assert_eq!(resolved[0].dissenting, vec![id("E001")]);
}

#[test]
fn staleness_uses_logical_ticks_so_the_answer_never_ages() {
    // The same question asked at the same tick must always give the same
    // answer. A wall clock would make this test fail tomorrow.
    let items = vec![
        evidence("E001", "sensor-a", 850, 900, 1_000),
        evidence("E002", "sensor-b", 850, 900, 1_000),
    ];
    let sources = sources(vec![source("sensor-a", None), source("sensor-b", None)]);

    for _ in 0..3 {
        let resolved = resolve_all(&items, &sources, &policy(), 1_050);
        assert_eq!(resolved[0].status, EpistemicStatus::Known);
    }
}

#[test]
fn resolution_is_declaration_order_independent() {
    let sources = sources(vec![
        source("sensor-a", None),
        source("sensor-b", None),
        source("sensor-c", None),
    ]);

    let forward = vec![
        evidence("E001", "sensor-a", 850, 900, 0),
        evidence("E002", "sensor-b", 850, 900, 0),
        evidence("E003", "sensor-c", 400, 200, 0),
    ];
    let mut reverse = forward.clone();
    reverse.reverse();

    let left = resolve_all(&forward, &sources, &policy(), 0);
    let right = resolve_all(&reverse, &sources, &policy(), 0);

    assert_eq!(left.len(), right.len());
    assert_eq!(left[0].value, right[0].value);
    assert_eq!(left[0].status, right[0].status);
    assert_eq!(left[0].supporting, right[0].supporting);
    assert_eq!(left[0].dissenting, right[0].dissenting);
}
