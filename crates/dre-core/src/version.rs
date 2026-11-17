//! Model and engine versioning.
//!
//! Version is part of the replay identity. Plan section 17 requires an
//! `EngineHash` alongside the model and input hashes, because a decision
//! is only reproducible relative to the engine that produced it. Two runs
//! that disagree are not necessarily a determinism bug — they may be two
//! different engines, and the hashes are what tell those cases apart.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::canonical::{Canonical, CanonicalWriter};
use crate::error::{CoreError, Result};

/// A `major.minor.patch` version.
///
/// Serialises as the string `"0.1.0"` rather than as a struct, because
/// that is how a model pack author writes it and how it reads back in a
/// diff. Ordering is by component significance, from the field order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    /// Incompatible changes to meaning.
    pub major: u32,
    /// Backwards-compatible additions.
    pub minor: u32,
    /// Fixes that do not change accepted input.
    pub patch: u32,
}

impl Version {
    /// Construct a version.
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a `major.minor.patch` string.
    ///
    /// Strict on purpose. Pre-release and build-metadata suffixes are
    /// rejected rather than ignored, because a silently discarded suffix
    /// would let two genuinely different models share a `ModelHash`.
    pub fn parse(value: &str) -> Result<Self> {
        fn invalid(value: &str, reason: &'static str) -> CoreError {
            CoreError::InvalidVersion {
                value: value.to_owned(),
                reason,
            }
        }

        let mut components = [0u32; 3];
        let mut parts = value.split('.');

        for slot in components.iter_mut() {
            let raw = parts
                .next()
                .ok_or_else(|| invalid(value, "expected exactly major.minor.patch"))?;
            *slot = raw
                .parse::<u32>()
                .map_err(|_| invalid(value, "components must be non-negative integers"))?;
        }

        if parts.next().is_some() {
            return Err(invalid(value, "expected exactly major.minor.patch"));
        }

        match components {
            [major, minor, patch] => Ok(Self::new(major, minor, patch)),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl TryFrom<String> for Version {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}

impl From<Version> for String {
    fn from(version: Version) -> Self {
        version.to_string()
    }
}

impl Canonical for Version {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("version")
            .write_u64(u64::from(self.major))
            .write_u64(u64::from(self.minor))
            .write_u64(u64::from(self.patch));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn parses_well_formed_versions() {
        assert_eq!(
            Version::parse("0.1.0").expect("valid"),
            Version::new(0, 1, 0)
        );
        assert_eq!(
            Version::parse("12.7.300").expect("valid"),
            Version::new(12, 7, 300)
        );
    }

    #[test]
    fn rejects_partial_and_decorated_versions() {
        for candidate in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.3-rc1",
            "1.2.3+build",
            "v1.2.3",
        ] {
            assert!(Version::parse(candidate).is_err(), "accepted {candidate:?}");
        }
    }

    #[test]
    fn orders_by_component_significance() {
        assert!(Version::new(0, 2, 0) > Version::new(0, 1, 9));
        assert!(Version::new(1, 0, 0) > Version::new(0, 9, 9));
    }

    #[test]
    fn round_trips_through_display() {
        let version = Version::new(3, 14, 15);
        assert_eq!(
            Version::parse(&version.to_string()).expect("valid"),
            version
        );
    }
}
