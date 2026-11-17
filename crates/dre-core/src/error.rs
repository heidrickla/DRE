//! Error types for the runtime foundation.
//!
//! Engine paths return errors rather than panicking. A panic mid-decision
//! leaves no proof and no replayable record, which defeats the point of
//! the engine; a returned error is itself a deterministic outcome.

use thiserror::Error;

/// Failures originating in the deterministic runtime foundation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// An identifier violated the character or length rules in [`crate::id`].
    #[error("invalid identifier {value:?}: {reason}")]
    InvalidId {
        /// The rejected identifier text.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A fixed-point operation exceeded the representable range.
    ///
    /// Deliberately an error rather than a wrap or a saturation: silently
    /// clamping would make the result depend on how close the inputs were
    /// to the boundary, which is exactly the kind of hidden discontinuity
    /// that makes a reasoning result hard to trust.
    #[error("fixed-point overflow in {operation}")]
    FixedOverflow {
        /// The arithmetic operation that overflowed.
        operation: &'static str,
    },

    /// A fixed-point division had a zero divisor.
    #[error("fixed-point division by zero")]
    FixedDivideByZero,

    /// A decimal string could not be read as a fixed-point value.
    #[error("invalid fixed-point value {value:?}: {reason}")]
    InvalidFixed {
        /// The rejected text.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// Canonical encoding or decoding failed.
    #[error("canonical encoding failed: {0}")]
    Canonical(String),

    /// A version string could not be parsed as `major.minor.patch`.
    #[error("invalid version {value:?}: {reason}")]
    InvalidVersion {
        /// The rejected version text.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A configured resource limit was exceeded.
    ///
    /// Limits are part of the engine identity: two runs with different
    /// limits are not the same engine and may legitimately differ, so
    /// hitting a limit must surface loudly rather than truncate silently.
    #[error("limit exceeded: {limit} (cap {cap})")]
    LimitExceeded {
        /// Name of the limit, matching a field in [`crate::config`].
        limit: &'static str,
        /// The configured ceiling.
        cap: u64,
    },
}

/// Result alias for the runtime foundation.
pub type Result<T> = std::result::Result<T, CoreError>;
