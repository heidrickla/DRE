//! Phase 1 — Deterministic runtime foundation.
//!
//! Every other crate in the workspace builds on the guarantees established
//! here. The contract this crate exists to uphold is:
//!
//! ```text
//! SameInput + SameModel + SameEngine = SameDecision
//! ```
//!
//! That is an invariant, not an aspiration. Anything in this crate that
//! admits ambient nondeterminism — iteration order, floating point, the
//! wall clock, thread scheduling, address-space layout — is a defect,
//! because it silently propagates into every decision the engine makes.
//!
//! See `docs/determinism.md` for the full prohibition list and the
//! reasoning behind each entry.

pub mod canonical;
pub mod collections;
pub mod config;
pub mod context;
pub mod error;
pub mod fixed;
pub mod hash;
pub mod id;
pub mod version;

pub use error::{CoreError, Result};
pub use hash::Hash32;
pub use id::Id;
