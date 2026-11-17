//! Immutable identifiers.
//!
//! Identifiers are the join keys of the whole engine: entities, rules,
//! regions, actions, constraints and goals are all referenced by `Id`.
//! Two properties matter.
//!
//! First, immutability. An `Id` cannot be mutated after construction, so
//! a structure keyed by one cannot silently reorder underneath a reader.
//!
//! Second, a total order that does not depend on the host. `Ord` here is
//! bytewise over the identifier text, not locale-aware collation, because
//! locale collation differs between machines and would break the
//! cross-machine determinism requirement in plan section 21.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Maximum identifier length.
///
/// Bounded so that a malformed or adversarial model pack cannot make
/// canonical encoding allocate without limit.
pub const MAX_ID_LEN: usize = 256;

/// An immutable, validated identifier.
///
/// Ordering is bytewise over the underlying ASCII text, giving the same
/// order on every platform.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(Box<str>);

impl Id {
    /// Construct an identifier, validating its shape.
    ///
    /// Accepts ASCII alphanumerics plus `-`, `_`, `.` and `:`. The charset
    /// is restricted rather than permissive so that identifiers are safe to
    /// embed directly in canonical encodings and proof output without
    /// escaping — an escaping layer would be one more place for two
    /// implementations to disagree.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value: String = value.into();

        if value.is_empty() {
            return Err(CoreError::InvalidId {
                value,
                reason: "identifier must not be empty",
            });
        }

        if value.len() > MAX_ID_LEN {
            return Err(CoreError::InvalidId {
                value,
                reason: "identifier exceeds MAX_ID_LEN",
            });
        }

        let valid = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));

        if !valid {
            return Err(CoreError::InvalidId {
                value,
                reason: "identifier may contain only ASCII alphanumerics and -_.:",
            });
        }

        Ok(Self(value.into_boxed_str()))
    }

    /// Borrow the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({:?})", self.0)
    }
}

impl TryFrom<String> for Id {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.0.into_string()
    }
}

#[cfg(test)]
mod tests {
    // The workspace denies these in engine paths because a panic mid-decision
    // leaves no proof. In tests a panic *is* the failure report, so the
    // restriction is lifted here rather than worked around.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn accepts_conventional_identifiers() {
        for candidate in ["R001", "object-a", "machine.temperature", "pack:logic"] {
            assert!(Id::new(candidate).is_ok(), "rejected {candidate}");
        }
    }

    #[test]
    fn rejects_empty_and_oversized() {
        assert!(Id::new("").is_err());
        assert!(Id::new("x".repeat(MAX_ID_LEN + 1)).is_err());
    }

    #[test]
    fn rejects_characters_needing_escaping() {
        for candidate in ["has space", "quote\"", "newline\n", "unicode-\u{e9}"] {
            assert!(Id::new(candidate).is_err(), "accepted {candidate:?}");
        }
    }

    #[test]
    fn orders_bytewise() {
        let mut ids = [
            Id::new("R010").expect("valid"),
            Id::new("R002").expect("valid"),
            Id::new("R001").expect("valid"),
        ];
        ids.sort();
        let rendered: Vec<&str> = ids.iter().map(Id::as_str).collect();
        assert_eq!(rendered, ["R001", "R002", "R010"]);
    }
}
