//! Canonical serialization.
//!
//! A hash only means something if the bytes going into it are uniquely
//! determined by the value. Most serializers are not: JSON may reorder
//! object keys, vary whitespace, or render the same number several ways,
//! and any of those differences changes the digest without changing the
//! meaning. Replay comparison would then report a mismatch for two runs
//! that actually agreed.
//!
//! The encoding here is deliberately dull and unambiguous:
//!
//! - integers are fixed-width big-endian, so target endianness cannot leak
//!   into the digest;
//! - byte strings are length-prefixed, so no separator can be forged by a
//!   value that happens to contain one;
//! - composites are tagged, so a struct cannot collide with a tuple whose
//!   fields encode identically;
//! - collections encode in the iteration order of a deterministic
//!   collection, which is their key order.
//!
//! This format is written for hashing, not for storage or interchange. It
//! has no reader. Snapshots that need to be read back use serde.

use crate::collections::DetMap;
use crate::fixed::Fixed;
use crate::hash::Hash32;
use crate::id::Id;

/// Accumulates canonical bytes.
#[derive(Clone, Debug, Default)]
pub struct CanonicalWriter {
    buffer: Vec<u8>,
}

impl CanonicalWriter {
    /// Start an empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Write a type tag.
    ///
    /// Distinguishes composites that would otherwise encode identically.
    pub fn tag(&mut self, tag: &str) -> &mut Self {
        self.write_str(tag)
    }

    /// Write a length-prefixed string.
    pub fn write_str(&mut self, value: &str) -> &mut Self {
        self.write_bytes(value.as_bytes())
    }

    /// Write a length-prefixed byte string.
    pub fn write_bytes(&mut self, value: &[u8]) -> &mut Self {
        self.write_u64(value.len() as u64);
        self.buffer.extend_from_slice(value);
        self
    }

    /// Write an unsigned 64-bit integer, big-endian.
    pub fn write_u64(&mut self, value: u64) -> &mut Self {
        self.buffer.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Write a signed 64-bit integer, big-endian two's complement.
    pub fn write_i64(&mut self, value: i64) -> &mut Self {
        self.buffer.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Write a boolean as a single byte.
    pub fn write_bool(&mut self, value: bool) -> &mut Self {
        self.buffer.push(u8::from(value));
        self
    }

    /// Write a nested canonical value.
    pub fn write<T: Canonical + ?Sized>(&mut self, value: &T) -> &mut Self {
        value.write_canonical(self);
        self
    }

    /// Write an optional value, tagged so that `None` and a present value
    /// can never encode alike.
    pub fn write_option<T: Canonical>(&mut self, value: Option<&T>) -> &mut Self {
        match value {
            None => self.write_bool(false),
            Some(inner) => {
                self.write_bool(true);
                self.write(inner)
            }
        }
    }

    /// Finish and take the accumulated bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }

    /// Borrow the accumulated bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }
}

/// A value with exactly one byte encoding.
pub trait Canonical {
    /// Append this value's canonical encoding.
    fn write_canonical(&self, out: &mut CanonicalWriter);

    /// Produce the canonical encoding standalone.
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut writer = CanonicalWriter::new();
        self.write_canonical(&mut writer);
        writer.finish()
    }

    /// Hash the canonical encoding.
    fn canonical_hash(&self) -> Hash32 {
        Hash32::of(&self.canonical_bytes())
    }
}

impl Canonical for str {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.write_str(self);
    }
}

impl Canonical for String {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.write_str(self);
    }
}

impl Canonical for bool {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.write_bool(*self);
    }
}

impl Canonical for i64 {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.write_i64(*self);
    }
}

impl Canonical for u64 {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.write_u64(*self);
    }
}

impl Canonical for Id {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("id").write_str(self.as_str());
    }
}

impl Canonical for Fixed {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("fixed").write_i64(self.raw());
    }
}

impl Canonical for Hash32 {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("hash").write_bytes(self.as_bytes());
    }
}

impl<T: Canonical> Canonical for Vec<T> {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("seq").write_u64(self.len() as u64);
        for item in self.iter() {
            out.write(item);
        }
    }
}

impl<T: Canonical> Canonical for Option<T> {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("opt").write_option(self.as_ref());
    }
}

impl<K: Canonical, V: Canonical> Canonical for DetMap<K, V> {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("map").write_u64(self.len() as u64);
        // Iteration order is key order, guaranteed by the ordered map.
        for (key, value) in self.iter() {
            out.write(key);
            out.write(value);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn same_value_encodes_identically() {
        let value = Fixed::from_raw(1_500_000);
        assert_eq!(value.canonical_bytes(), value.canonical_bytes());
    }

    #[test]
    fn map_encoding_is_insertion_order_independent() {
        let mut forward: DetMap<String, i64> = DetMap::new();
        forward.insert("alpha".to_owned(), 1);
        forward.insert("beta".to_owned(), 2);
        forward.insert("gamma".to_owned(), 3);

        let mut reverse: DetMap<String, i64> = DetMap::new();
        reverse.insert("gamma".to_owned(), 3);
        reverse.insert("beta".to_owned(), 2);
        reverse.insert("alpha".to_owned(), 1);

        // Plan section 21 requires that randomised input ordering not change
        // the result. This is that property at the encoding layer.
        assert_eq!(forward.canonical_hash(), reverse.canonical_hash());
    }

    #[test]
    fn length_prefixing_prevents_boundary_forgery() {
        // Without a length prefix, ["ab", "c"] and ["a", "bc"] would both
        // encode as the bytes "abc" and collide.
        let left = vec!["ab".to_owned(), "c".to_owned()];
        let right = vec!["a".to_owned(), "bc".to_owned()];
        assert_ne!(left.canonical_hash(), right.canonical_hash());
    }

    #[test]
    fn absent_and_present_options_differ() {
        let absent: Option<i64> = None;
        let present: Option<i64> = Some(0);
        assert_ne!(absent.canonical_hash(), present.canonical_hash());
    }
}
