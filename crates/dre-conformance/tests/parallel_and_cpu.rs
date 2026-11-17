//! Concurrency attacks from plan section 21.
//!
//! Two items on that list — "different CPU counts" and "parallel execution"
//! — look vacuous at first, because nothing in the engine is parallel. That
//! reading is wrong in an instructive way, and the distinction is worth
//! writing down rather than skipping the tests.
//!
//! # What running concurrently actually tests
//!
//! The engine being single-threaded internally says nothing about what
//! happens when several runs exist at once. The failure modes are real and
//! all of them are invisible in a sequential test:
//!
//! - a `static mut`, `OnceLock` cache, or lazily-initialised table shared
//!   between runs, so the second run sees the first run's leftovers;
//!   whichever run got there first would then decide for both,
//! - a thread-local memo that makes a result depend on which worker picked
//!   the job up,
//! - iteration over something whose order derives from an allocator address,
//!   which differs between threads because their arenas differ.
//!
//! Any of those turns "same input, same decision" into "same input, same
//! decision if nothing else was running". Since the whole point of the
//! project is a claim about reproducibility, that qualifier has to be
//! disproved rather than assumed.
//!
//! # What "different CPU counts" can honestly be tested as
//!
//! A process cannot portably reduce its own CPU count from inside a test,
//! and shelling out to `taskset` would test the platform more than the
//! engine. So the item is covered from both ends instead:
//!
//! - **Behaviourally** — the same work is spread across 1, 2, 3, 8 and 16
//!   threads. If the engine's answer depended on how much concurrency was
//!   available, the width would be the thing that varied it.
//! - **Structurally** — [`no_engine_crate_observes_the_machine`] proves no
//!   engine crate can even *ask* how many CPUs there are. A result cannot
//!   depend on a number nothing reads.
//!
//! Neither is the same as running on a machine with a different core count,
//! and this file does not claim it is. What they rule out is the mechanism
//! by which core count could reach a decision. Running under `taskset -c 0`
//! remains a worthwhile manual check; see `docs/verification.md`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn experiment(name: &str) -> PathBuf {
    repo_root().join("experiments").join(name)
}

const EXPERIMENTS: [&str; 10] = [
    "logic-001.yaml",
    "diagnosis-001.yaml",
    "diagnosis-002.yaml",
    "diagnosis-003.yaml",
    "navigation-001.yaml",
    "process-001.yaml",
    "process-002.yaml",
    "grid-40-001.yaml",
    "cold-chain-release-001.yaml",
    "cold-chain-release-002.yaml",
];

/// The two hashes that must not move: what was decided, and how.
fn fingerprint(name: &str) -> (String, String) {
    let result = dre_lab::run(&experiment(name)).expect("experiment runs");
    (
        result.record.decision.to_string(),
        result.record.proof.to_string(),
    )
}

/// Run `jobs` experiments spread across `threads` workers.
///
/// Work is handed out by index rather than by chunk, so consecutive jobs
/// land on different workers and the experiments genuinely interleave. A
/// chunked split would run each experiment on one thread and never overlap
/// two different problems in flight at once.
fn run_across(threads: usize, jobs: &[&'static str]) -> Vec<(String, (String, String))> {
    let collected = std::sync::Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for worker in 0..threads {
            let collected = &collected;
            scope.spawn(move || {
                for (index, name) in jobs.iter().enumerate() {
                    if index % threads != worker {
                        continue;
                    }
                    let outcome = fingerprint(name);
                    collected
                        .lock()
                        .expect("no worker panicked while holding the lock")
                        .push(((*name).to_owned(), outcome));
                }
            });
        }
    });

    collected.into_inner().expect("workers finished")
}

#[test]
fn concurrent_runs_agree_with_the_sequential_ones() {
    // The baseline is taken first, alone, so it is unambiguously what the
    // engine decides when nothing else is happening.
    let baseline: BTreeMap<String, (String, String)> = EXPERIMENTS
        .iter()
        .map(|name| ((*name).to_owned(), fingerprint(name)))
        .collect();

    // Every experiment several times over, so runs of *different* problems
    // overlap rather than just runs of the same one.
    let jobs: Vec<&'static str> = EXPERIMENTS.iter().copied().cycle().take(32).collect();

    for observed in run_across(8, &jobs) {
        let (name, outcome) = observed;
        assert_eq!(
            baseline.get(&name),
            Some(&outcome),
            "{name} decided differently when run concurrently"
        );
    }
}

#[test]
fn the_number_of_workers_does_not_reach_the_result() {
    let jobs: Vec<&'static str> = EXPERIMENTS.iter().copied().cycle().take(16).collect();

    let mut seen: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();

    for threads in [1usize, 2, 3, 8, 16] {
        for (name, outcome) in run_across(threads, &jobs) {
            seen.entry(name).or_default().insert(outcome);
        }
    }

    assert_eq!(seen.len(), EXPERIMENTS.len(), "not every experiment ran");
    for (name, outcomes) in &seen {
        assert_eq!(
            outcomes.len(),
            1,
            "{name} produced {} different results across thread counts",
            outcomes.len()
        );
    }
}

#[test]
fn a_single_experiment_survives_heavy_contention() {
    // Narrower and harsher than the test above: one experiment, many
    // threads, all hammering the same code paths at once. This is the shape
    // that finds a shared cache -- the runs are identical, so a leaked
    // value from one is indistinguishable from a correct one in another
    // until the cache holds something stale.
    let expected = fingerprint("diagnosis-002.yaml");
    let jobs: Vec<&'static str> = vec!["diagnosis-002.yaml"; 64];

    let outcomes: BTreeSet<(String, String)> = run_across(16, &jobs)
        .into_iter()
        .map(|(_, outcome)| outcome)
        .collect();

    assert_eq!(outcomes.len(), 1, "64 concurrent runs disagreed");
    assert_eq!(
        outcomes.into_iter().next(),
        Some(expected),
        "concurrent runs agreed with each other but not with the sequential result"
    );
}

// ---------------------------------------------------------------------
// The structural half of "different CPU counts".
// ---------------------------------------------------------------------

/// Ways a crate could learn how much machine it is running on.
///
/// Spelled as source tokens rather than as a policy in a document, because
/// a rule that only exists in prose is one commit from being false. This is
/// the same tactic `clippy.toml` uses for `HashMap` and `Instant`; those
/// have lints available and these do not.
const MACHINE_PROBES: [&str; 6] = [
    "available_parallelism",
    "num_cpus",
    "rayon",
    "std::thread",
    "thread::spawn",
    "std::env",
];

/// Crates permitted to look, and why.
///
/// `dre-lab` is the harness: it is allowed to measure, and plan section 20
/// requires it to. `dre-conformance` is this suite, which has to spawn
/// threads in order to attack the engine with them.
const EXEMPT: [&str; 2] = ["dre-lab", "dre-conformance"];

#[test]
fn no_engine_crate_observes_the_machine() {
    let crates_dir = repo_root().join("crates");
    let mut checked = 0usize;
    let mut findings = Vec::new();

    for entry in std::fs::read_dir(&crates_dir).expect("crates/ is readable") {
        let path = entry.expect("directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();

        if !path.is_dir() || EXEMPT.contains(&name.as_str()) {
            continue;
        }

        for file in rust_sources(&path.join("src")) {
            checked += 1;
            let text = std::fs::read_to_string(&file).expect("source is readable");
            for (number, line) in code_lines(&text) {
                for probe in MACHINE_PROBES {
                    if line.contains(probe) {
                        findings.push(format!("{name}: {}:{number} uses {probe}", file.display()));
                    }
                }
            }
        }
    }

    // Without this the test would pass loudly on a broken path lookup,
    // reporting no findings because it read no files.
    assert!(
        checked > 10,
        "only scanned {checked} files; the crate walk is not finding sources"
    );

    assert!(
        findings.is_empty(),
        "engine crates must not be able to observe the machine they run on:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_scan_catches_what_it_is_looking_for() {
    // Positive control. A fitness test that has never been shown to fail is
    // indistinguishable from one that cannot.
    let offending = "fn width() -> usize {\n    std::thread::available_parallelism().unwrap()\n}\n";

    let caught = code_lines(offending)
        .any(|(_, line)| MACHINE_PROBES.into_iter().any(|probe| line.contains(probe)));

    assert!(caught, "the scan missed a direct CPU-count probe");
}

#[test]
fn the_scan_reads_code_rather_than_prose() {
    // Several engine modules explain at length why they do not read the
    // clock, the environment or the core count. If discussing the thing
    // tripped the scan, the only way to keep it green would be to stop
    // writing the explanations down.
    let commented = "//! Deliberately no `std::thread` and no `available_parallelism` here.\n\
                     /// Not `num_cpus` either.\n\
                     // rayon is banned.\n";

    assert_eq!(
        code_lines(commented).count(),
        0,
        "comment lines should not reach the scan at all"
    );
}

/// Every `.rs` file under a directory, recursively.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }

    found.sort();
    found
}

/// Numbered lines with comment-only lines dropped.
///
/// The prohibition is on *reaching for* the machine, not on discussing it,
/// and several engine modules explain at length why they do not read the
/// clock or the environment. A scan that could not tell those apart would
/// have to be silenced with markers, which is how a fitness test becomes
/// noise. Line comments are all this codebase uses; a block comment would
/// slip through, which is a known and accepted limit.
fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
}
