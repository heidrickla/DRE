//! The benchmark archive.
//!
//! `dre benchmark` measures a pack and prints the plan section 20 metrics.
//! Printed and discarded, those answer "how fast is it" and never "has it
//! got slower" — and the second question is the one a benchmark exists
//! for. So each run appends a row here.
//!
//! # Why JSONL rather than SQLite
//!
//! Plan section 5 suggests SQLite for experiment results, and
//! [`crate::runner`]'s replay records already departed from that for
//! readability. This is a different artefact with a different argument.
//!
//! What SQLite would buy is indexed queries and aggregation over many
//! rows. What it costs is a C dependency in a workspace whose stated
//! policy is that every dependency is a risk surface needing justification
//! — and this archive is append-only, never updated in place, and read
//! back a few hundred rows at a time. `jq` and `awk` handle that. A
//! dependency earning its place at ten thousand rows does not earn it at
//! two hundred.
//!
//! The line-per-run format also matters for what it prevents: appending
//! cannot corrupt earlier rows, so a benchmark interrupted halfway leaves
//! the history it had rather than a file that will not open.
//!
//! # Why it is not committed
//!
//! Replay records *are* committed, because a decision hash is a claim
//! anyone can check and a changed one should be a visible line in review.
//! A timing is not that kind of claim. It depends on the machine, the
//! build profile, and whatever else was running, so a committed timing
//! would carry an authority it has not got, and the diff noise would train
//! people to skip it.
//!
//! The archive is therefore per-machine and gitignored — which is also the
//! only honest scope for it, since two rows from different hosts are not
//! comparable and nothing in the file could tell you so.

use std::path::{Path, PathBuf};

use dre_core::context::Counters;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Directory holding benchmark archives, relative to the repository root.
pub const ARCHIVE_DIR: &str = "experiments/benchmarks";

/// Failures reading or writing the archive.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// Filesystem access failed.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A row was not valid JSON.
    ///
    /// Reported rather than skipped. A parser that silently dropped
    /// unreadable rows would make a corrupted archive look like a short
    /// one, and the whole point of the file is counting what happened.
    #[error("{path}:{line}: {source}")]
    Malformed {
        /// The path.
        path: PathBuf,
        /// Line number, 1-based.
        line: usize,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// One benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRecord {
    /// The experiment measured.
    pub experiment: Id,

    /// Unix seconds when the run finished.
    ///
    /// Recorded by the harness, never seen by the engine. It exists to
    /// order rows in a report, and ordering is the one thing a history
    /// cannot do without.
    pub recorded_at_unix: u64,

    /// `debug` or `release`.
    ///
    /// Load-bearing: debug and release timings differ by an order of
    /// magnitude, so comparing across profiles would manufacture
    /// regressions and hide real ones. Rows are grouped by this.
    pub profile: String,

    /// Iterations in the loop.
    pub iterations: u32,

    /// Distinct decision hashes seen. Anything but 1 is a determinism
    /// failure.
    pub unique_decisions: usize,
    /// Distinct proof hashes seen.
    pub unique_proofs: usize,

    /// The decision hash, when there was exactly one.
    pub decision: Option<Hash32>,
    /// The proof hash, when there was exactly one.
    pub proof: Option<Hash32>,

    /// Model hash. Rows from a different model are not comparable.
    pub model: Hash32,
    /// Engine hash. Same.
    pub engine: Hash32,
    /// Engine version that produced the row.
    pub engine_version: String,

    /// Mean wall-clock per iteration, microseconds.
    pub mean_micros: u64,
    /// Peak process RSS at the end of the loop.
    pub peak_memory_kib: Option<u64>,
    /// How much that peak rose during the loop.
    pub memory_growth_kib: Option<u64>,
    /// Proof size in nodes.
    pub proof_nodes: usize,
    /// Workload counters from the final iteration.
    pub counters: Counters,
}

/// Path an experiment's archive lives at.
#[must_use]
pub fn archive_path(root: &Path, experiment: &Id) -> PathBuf {
    root.join(ARCHIVE_DIR).join(format!("{experiment}.jsonl"))
}

/// Append one row.
pub fn append(root: &Path, record: &BenchmarkRecord) -> Result<PathBuf, ArchiveError> {
    use std::io::Write as _;

    let path = archive_path(root, &record.experiment);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ArchiveError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    // Serialised to one line deliberately. A pretty-printed row would make
    // the file no longer append-only in any useful sense -- you could not
    // tell where one run ended without parsing.
    let mut line = serde_json::to_string(record).unwrap_or_default();
    line.push('\n');

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| ArchiveError::Io {
            path: path.clone(),
            source,
        })?;

    file.write_all(line.as_bytes())
        .map_err(|source| ArchiveError::Io {
            path: path.clone(),
            source,
        })?;

    Ok(path)
}

/// Read every row for an experiment, oldest first.
///
/// An absent archive is an empty history, not an error — nothing has been
/// measured yet, which is a normal state and not a fault.
pub fn load(root: &Path, experiment: &Id) -> Result<Vec<BenchmarkRecord>, ArchiveError> {
    let path = archive_path(root, experiment);

    if !path.is_file() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(&path).map_err(|source| ArchiveError::Io {
        path: path.clone(),
        source,
    })?;

    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(line).map_err(|source| ArchiveError::Malformed {
            path: path.clone(),
            line: index + 1,
            source,
        })?;
        records.push(record);
    }

    Ok(records)
}

/// What a history says about one experiment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    /// Rows considered, after filtering to the newest row's profile.
    pub considered: usize,
    /// Rows set aside as not comparable, and why.
    pub excluded: Vec<String>,
    /// Distinct decision hashes across the considered rows.
    pub decisions: usize,
    /// Distinct proof hashes across the considered rows.
    pub proofs: usize,
    /// Mean execution of the newest row.
    pub latest_micros: u64,
    /// Mean execution of the row before it, when there is one.
    pub previous_micros: Option<u64>,
    /// Fastest and slowest means seen.
    pub fastest_micros: u64,
    /// Slowest mean seen.
    pub slowest_micros: u64,
}

impl History {
    /// Change from the previous row to the latest, in parts per thousand.
    ///
    /// `None` when there is nothing to compare against. Per-mille integers
    /// rather than a ratio, because floating point is denied workspace-wide
    /// and the reporting path should not be the one exception to that.
    ///
    /// Positive means slower.
    #[must_use]
    pub fn drift_permille(&self) -> Option<i64> {
        let previous = self.previous_micros?;
        if previous == 0 {
            return None;
        }
        let latest = i64::try_from(self.latest_micros).unwrap_or(i64::MAX);
        let previous = i64::try_from(previous).unwrap_or(i64::MAX);
        Some(
            latest
                .saturating_sub(previous)
                .saturating_mul(1_000)
                .checked_div(previous)
                .unwrap_or(0),
        )
    }

    /// Whether every considered row agreed on one decision and one proof.
    ///
    /// This is the archive's real payload. A benchmark that reported a
    /// timing while the decision hash had quietly moved across the history
    /// would be measuring the speed of the wrong answer.
    #[must_use]
    pub const fn agrees(&self) -> bool {
        self.decisions <= 1 && self.proofs <= 1
    }
}

/// Summarise an archive, comparing only rows that can be compared.
///
/// Rows are filtered to the newest row's build profile, model hash and
/// engine hash. Everything else is listed as excluded rather than dropped
/// quietly: a history that silently ignored half its rows would report a
/// steady trend across a change that moved the ground under it.
#[must_use]
pub fn summarize(records: &[BenchmarkRecord]) -> Option<History> {
    let latest = records.last()?;

    let mut considered = Vec::new();
    let mut excluded = Vec::new();

    // Rows are identified by position as well as timestamp: several runs
    // inside one second share a timestamp, and three identical exclusion
    // lines say nothing about which rows they are.
    for (index, record) in records.iter().enumerate() {
        let row = format!("row {} (t={})", index + 1, record.recorded_at_unix);

        if record.profile != latest.profile {
            excluded.push(format!(
                "{row} built in {} profile, latest is {}",
                record.profile, latest.profile
            ));
        } else if record.model != latest.model {
            excluded.push(format!(
                "{row} used model {}, latest is {}",
                record.model.short(),
                latest.model.short()
            ));
        } else if record.engine != latest.engine {
            excluded.push(format!(
                "{row} used engine {}, latest is {}",
                record.engine.short(),
                latest.engine.short()
            ));
        } else {
            considered.push(record);
        }
    }

    let mut decisions = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut fastest = u64::MAX;
    let mut slowest = 0u64;

    for record in &considered {
        if let Some(decision) = record.decision {
            decisions.insert(decision);
        }
        if let Some(proof) = record.proof {
            proofs.insert(proof);
        }
        fastest = fastest.min(record.mean_micros);
        slowest = slowest.max(record.mean_micros);
    }

    // The comparison row is the one before the newest *among comparable
    // rows*, not the one before it in the file.
    let previous_micros = considered
        .len()
        .checked_sub(2)
        .and_then(|index| considered.get(index))
        .map(|record| record.mean_micros);

    Some(History {
        considered: considered.len(),
        excluded,
        decisions: decisions.len(),
        proofs: proofs.len(),
        latest_micros: latest.mean_micros,
        previous_micros,
        fastest_micros: if fastest == u64::MAX { 0 } else { fastest },
        slowest_micros: slowest,
    })
}

/// Seconds since the Unix epoch.
///
/// The harness may read the clock; the engine may not. Nothing this
/// returns is handed back into a reasoning path — it orders rows in a
/// report and nothing else.
#[must_use]
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Which build produced a row.
#[must_use]
pub const fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn record(at: u64, micros: u64, profile: &str, decision: u8) -> BenchmarkRecord {
        BenchmarkRecord {
            experiment: Id::new("test-001").expect("valid id"),
            recorded_at_unix: at,
            profile: profile.to_owned(),
            iterations: 100,
            unique_decisions: 1,
            unique_proofs: 1,
            decision: Some(Hash32::of(&[decision])),
            proof: Some(Hash32::of(b"proof")),
            model: Hash32::of(b"model"),
            engine: Hash32::of(b"engine"),
            engine_version: "0.1.0".to_owned(),
            mean_micros: micros,
            peak_memory_kib: Some(4_096),
            memory_growth_kib: Some(0),
            proof_nodes: 7,
            counters: Counters::default(),
        }
    }

    #[test]
    fn an_empty_archive_summarises_to_nothing() {
        assert_eq!(summarize(&[]), None);
    }

    #[test]
    fn drift_is_measured_against_the_previous_comparable_row() {
        let history = summarize(&[record(1, 200, "release", 1), record(2, 220, "release", 1)])
            .expect("two rows");

        assert_eq!(history.considered, 2);
        assert_eq!(history.latest_micros, 220);
        assert_eq!(history.previous_micros, Some(200));
        // 20 slower on 200 is exactly 10%.
        assert_eq!(history.drift_permille(), Some(100));
        assert!(history.agrees());
    }

    #[test]
    fn a_single_row_has_nothing_to_drift_from() {
        let history = summarize(&[record(1, 200, "release", 1)]).expect("one row");
        assert_eq!(history.previous_micros, None);
        assert_eq!(history.drift_permille(), None);
    }

    #[test]
    fn getting_faster_reports_a_negative_drift() {
        let history = summarize(&[record(1, 200, "release", 1), record(2, 150, "release", 1)])
            .expect("two rows");
        assert_eq!(history.drift_permille(), Some(-250));
    }

    #[test]
    fn debug_rows_are_excluded_rather_than_averaged_in() {
        // The failure this prevents: one debug row among release rows
        // would move the mean by an order of magnitude and read as a
        // catastrophic regression that never happened.
        let history = summarize(&[
            record(1, 3_000, "debug", 1),
            record(2, 200, "release", 1),
            record(3, 210, "release", 1),
        ])
        .expect("rows present");

        assert_eq!(history.considered, 2);
        assert_eq!(history.excluded.len(), 1);
        assert!(history.excluded[0].contains("debug"));
        assert_eq!(history.previous_micros, Some(200));
        assert_eq!(
            history.slowest_micros, 210,
            "the debug row is not the slowest"
        );
    }

    #[test]
    fn a_changed_decision_across_the_history_is_reported() {
        // Each row was internally deterministic -- one decision hash over
        // its own loop -- and yet the answer changed between them. A
        // per-row check cannot see this; only the history can.
        let history = summarize(&[record(1, 200, "release", 1), record(2, 200, "release", 2)])
            .expect("two rows");

        assert_eq!(history.decisions, 2);
        assert!(
            !history.agrees(),
            "two decision hashes across the history must not read as agreement"
        );
    }

    #[test]
    fn rows_from_another_model_are_set_aside() {
        let mut older = record(1, 200, "release", 1);
        older.model = Hash32::of(b"a different pack");

        let history = summarize(&[older, record(2, 210, "release", 1)]).expect("rows present");

        assert_eq!(history.considered, 1);
        assert_eq!(history.excluded.len(), 1);
        assert!(history.excluded[0].contains("model"));
        assert_eq!(
            history.previous_micros, None,
            "nothing comparable to compare to"
        );
    }

    #[test]
    fn a_row_survives_the_round_trip_it_is_stored_through() {
        let original = record(1, 200, "release", 1);
        let line = serde_json::to_string(&original).expect("serialises");

        assert!(!line.contains('\n'), "a row must be one line");

        let restored: BenchmarkRecord = serde_json::from_str(&line).expect("deserialises");
        assert_eq!(restored.mean_micros, original.mean_micros);
        assert_eq!(restored.decision, original.decision);
        assert_eq!(restored.profile, original.profile);
    }

    #[test]
    fn the_profile_matches_the_build() {
        // Guards against the cfg going stale: if this ever disagreed with
        // reality, every history would silently compare across profiles.
        if cfg!(debug_assertions) {
            assert_eq!(profile(), "debug");
        } else {
            assert_eq!(profile(), "release");
        }
    }
}
