//! Phase 1 acceptance test (plan section 7).
//!
//! ```text
//! Run the same workload 100,000 times
//! and verify: unique output hashes = 1
//! ```
//!
//! This is the deliverable of phase 1. Everything in `dre-core` is
//! scaffolding until it passes.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use dre_conformance::{phase1_workload, Permuter};

/// Iterations for the acceptance test.
///
/// Overridable so the suite can be run harder than the plan's floor, but
/// never softer: a value below 100,000 is rejected rather than quietly
/// accepted, because a "passing" acceptance test that ran 100 iterations
/// would be worse than no test at all.
fn iterations() -> u64 {
    const REQUIRED: u64 = 100_000;
    match std::env::var("DRE_CONFORMANCE_ITERATIONS") {
        Ok(raw) => {
            let requested: u64 = raw
                .parse()
                .expect("DRE_CONFORMANCE_ITERATIONS must be an integer");
            assert!(
                requested >= REQUIRED,
                "DRE_CONFORMANCE_ITERATIONS={requested} is below the plan's floor of {REQUIRED}"
            );
            requested
        }
        Err(_) => REQUIRED,
    }
}

#[test]
fn same_workload_100k_times_yields_one_hash() {
    let total = iterations();
    let mut hashes: BTreeSet<String> = BTreeSet::new();
    let mut first_divergence: Option<u64> = None;

    for seed in 0..total {
        let hash = phase1_workload(seed).expect("workload must not fail");
        if hashes.insert(hash.to_string()) && hashes.len() > 1 && first_divergence.is_none() {
            first_divergence = Some(seed);
        }
    }

    assert_eq!(
        hashes.len(),
        1,
        "expected exactly 1 unique hash across {total} iterations, got {}. \
         First divergence at seed {:?}. Replay it with \
         `phase1_workload({:?})` — the seed fully determines the \
         construction order, so the failure is reproducible.",
        hashes.len(),
        first_divergence,
        first_divergence,
    );
}

/// Guards the test above against being vacuous.
///
/// If the permuter did not actually permute, every iteration would build
/// identical structures in an identical order and the acceptance test
/// would pass while proving nothing. This asserts the variation the
/// acceptance test depends on is real.
#[test]
fn the_acceptance_test_is_not_vacuous() {
    let baseline: Vec<u32> = (0..24).collect();

    let permute = |seed: u64| {
        let mut items = baseline.clone();
        Permuter::new(seed).shuffle(&mut items);
        items
    };

    let orders: BTreeSet<Vec<u32>> = (0..64).map(permute).collect();

    assert!(
        orders.len() > 32,
        "the permuter produced only {} distinct orders from 64 seeds — \
         the acceptance test would not be exercising ordering at all",
        orders.len()
    );

    // A shuffle must reorder, not corrupt: same multiset, different order.
    let shuffled = permute(1);
    let mut sorted = shuffled.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, baseline, "shuffle changed the contents");
    assert_ne!(shuffled, baseline, "shuffle left the order untouched");
}

/// Construction order must not reach the output.
///
/// The same assertion as the acceptance test at a scale that runs fast
/// enough to be useful while iterating.
#[test]
fn declaration_order_does_not_reach_the_hash() {
    let reference = phase1_workload(0).expect("workload must not fail");
    for seed in 1..1_000 {
        let hash = phase1_workload(seed).expect("workload must not fail");
        assert_eq!(
            hash, reference,
            "seed {seed} produced a different hash — construction order leaked into the output"
        );
    }
}

/// The workload must be stable within a process, not merely internally
/// consistent across seeds. A workload that returned a per-process
/// constant would satisfy the ordering tests and still be useless.
#[test]
fn workload_is_stable_on_repeat() {
    let first = phase1_workload(42).expect("workload must not fail");
    let second = phase1_workload(42).expect("workload must not fail");
    assert_eq!(first, second);
}
