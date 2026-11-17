//! On-disk replay records.
//!
//! Until these existed, `dre replay` re-ran an experiment twice and
//! compared the two runs — which proves the engine agrees with *itself
//! right now*, and says nothing about whether it still agrees with what it
//! decided last month. That is the case replay records exist for.
//!
//! # Why JSON files rather than SQLite
//!
//! Plan section 5 suggests SQLite for experiment results. Records go to
//! committed JSON instead, and the reason is the project's own subject: a
//! replay record is a claim about what the engine decided, and a claim
//! nobody can read is a claim nobody can check. A JSON file diffs in a pull
//! request, survives a tool going away, and makes a changed decision hash a
//! visible line in review rather than a row in a binary nobody opens.
//!
//! SQLite still earns its place for *benchmark* results — thousands of
//! timing rows are not a thing anyone reads by eye. That is a different
//! artefact and is still outstanding.

use std::path::{Path, PathBuf};

use dre_core::id::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ReplayRecord;

/// Directory holding stored records, relative to the repository root.
pub const RECORDS_DIR: &str = "experiments/records";

/// Failures reading or writing records.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Filesystem access failed.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The record file was not valid JSON, or not a record.
    #[error("{path}: {source}")]
    Malformed {
        /// The path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// No record has been stored for this experiment yet.
    #[error("no stored record for {experiment} (run `dre run {experiment} --record` first)")]
    Absent {
        /// The experiment.
        experiment: Id,
    },
}

/// A stored record, plus the metadata needed to interpret a mismatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRecord {
    /// The five hashes.
    #[serde(flatten)]
    pub record: ReplayRecord,

    /// Engine version that produced it.
    ///
    /// Present so a mismatch can say *why*. A differing `EngineHash` alone
    /// tells you the engine changed; the version tells you which one you
    /// were comparing against.
    pub engine_version: String,

    /// Free-text note from whoever recorded it.
    #[serde(default)]
    pub note: String,
}

/// Path a record is stored at.
#[must_use]
pub fn record_path(root: &Path, experiment: &Id) -> PathBuf {
    root.join(RECORDS_DIR).join(format!("{experiment}.json"))
}

/// Write a record, creating the directory if needed.
pub fn save(root: &Path, stored: &StoredRecord) -> Result<PathBuf, StoreError> {
    let path = record_path(root, &stored.record.experiment);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    // Pretty-printed with a trailing newline: this file is meant to be
    // read in a diff, and a one-line JSON blob is not.
    let mut text =
        serde_json::to_string_pretty(stored).map_err(|source| StoreError::Malformed {
            path: path.clone(),
            source,
        })?;
    text.push('\n');

    std::fs::write(&path, text).map_err(|source| StoreError::Io {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

/// Read a stored record.
pub fn load(root: &Path, experiment: &Id) -> Result<StoredRecord, StoreError> {
    let path = record_path(root, experiment);

    if !path.is_file() {
        return Err(StoreError::Absent {
            experiment: experiment.clone(),
        });
    }

    let text = std::fs::read_to_string(&path).map_err(|source| StoreError::Io {
        path: path.clone(),
        source,
    })?;

    serde_json::from_str(&text).map_err(|source| StoreError::Malformed { path, source })
}

/// Whether a record exists.
#[must_use]
pub fn exists(root: &Path, experiment: &Id) -> bool {
    record_path(root, experiment).is_file()
}
