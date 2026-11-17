//! Deterministic hashing.
//!
//! Hashes are the engine's evidence that a run is reproducible. The five
//! hashes in plan section 17 — input, model, engine, decision, proof —
//! are all produced here, over canonical bytes.
//!
//! BLAKE3 is used because it is a fixed, specified function: the same
//! bytes give the same digest on every platform and every version. This is
//! deliberately *not* `std::hash::Hasher`, whose output is explicitly not
//! guaranteed stable across releases or processes.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A 256-bit BLAKE3 digest.
///
/// Serialises as a 64-character hex string, not as an array of 32 numbers.
/// Stored replay records are meant to be read and diffed by people — a
/// changed digest should be one obviously different line, not thirty-two
/// subtly different ones.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32([u8; 32]);

impl Serialize for Hash32 {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hash32 {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("expected 64 hex characters, got {:?}", text))
        })
    }
}

impl Hash32 {
    /// Hash a byte slice.
    ///
    /// Callers should pass canonical bytes (see [`crate::canonical`]).
    /// Hashing a non-canonical encoding gives a digest that depends on
    /// incidental formatting, which silently breaks replay comparison.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Borrow the raw digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Construct from raw digest bytes, for decoding stored records.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parse a 64-character hex digest, as written by [`Display`].
    ///
    /// [`Display`]: std::fmt::Display
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        fn nibble(byte: u8) -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            }
        }

        if text.len() != 64 {
            return None;
        }

        let mut bytes = [0u8; 32];
        for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
            // Slice pattern rather than indexing: the pattern is the
            // guarantee that the chunk is a full pair.
            let [high, low] = pair else { return None };
            *slot = (nibble(*high)? << 4) | nibble(*low)?;
        }

        Some(Self(bytes))
    }

    /// Render the leading bytes as hex, for human-facing output.
    ///
    /// Short form is for logs and CLI summaries only. Replay verification
    /// always compares the full digest.
    #[must_use]
    pub fn short(&self) -> String {
        self.to_string().chars().take(12).collect()
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter() {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({self})")
    }
}

/// Incremental hasher, for digests built from many parts.
#[derive(Clone, Default)]
pub struct Hasher(blake3::Hasher);

impl Hasher {
    /// Start an empty hasher.
    #[must_use]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    /// Absorb more canonical bytes.
    ///
    /// Order matters. Callers feeding a collection must iterate a
    /// deterministic collection (see [`crate::collections`]) or sort first.
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(bytes);
        self
    }

    /// Finish and produce the digest.
    #[must_use]
    pub fn finish(&self) -> Hash32 {
        Hash32(*self.0.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stable_for_identical_input() {
        assert_eq!(Hash32::of(b"reasoning"), Hash32::of(b"reasoning"));
    }

    #[test]
    fn separates_distinct_input() {
        assert_ne!(Hash32::of(b"reasoning"), Hash32::of(b"reasoning "));
    }

    #[test]
    fn incremental_matches_oneshot() {
        let mut hasher = Hasher::new();
        hasher.update(b"reason").update(b"ing");
        assert_eq!(hasher.finish(), Hash32::of(b"reasoning"));
    }

    #[test]
    fn renders_as_64_hex_characters() {
        let rendered = Hash32::of(b"reasoning").to_string();
        assert_eq!(rendered.len(), 64);
        assert!(rendered.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
