//! DRE Lab — the harness around the engine.
//!
//! This is the one crate where the determinism rules deliberately stop. It
//! reads the filesystem, measures elapsed time, and writes reports. What it
//! must never do is feed any of that back into a reasoning path: a timing
//! that cannot be observed by the engine cannot change an answer.
//!
//! The library lives beside the `dre` binary so experiments can be driven
//! from tests as well as from the command line.
//!
//! # The clock exemption
//!
//! `clippy.toml` bans `Instant` and `SystemTime` workspace-wide, and the
//! ban fires here too — correctly, because the lint has no way to know
//! which side of the boundary it is on. The allow below is that boundary
//! made explicit, and it is deliberately at the crate root rather than
//! buried at the call site: the exemption should be visible in exactly one
//! place, and it should be this crate.
//!
//! Plan section 20 requires execution time as a benchmark metric, so the
//! harness has to measure it. The rule it must keep is not "never read the
//! clock" but "never let a reading reach a decision" — the engine crates
//! have no way to observe what is measured here.

#![allow(clippy::disallowed_types)]

pub mod bench;
pub mod counterfactual;
pub mod experiment;
pub mod memory;
pub mod runner;
pub mod survey;
pub mod validate;

pub use experiment::{files_in as experiment_files_in, Experiment, ExperimentFile};
pub use runner::{load_experiment, run, run_experiment, RunError, RunResult};
