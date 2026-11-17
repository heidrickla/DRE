//! Fixed-point arithmetic.
//!
//! Floating point is banned in engine paths (enforced by the workspace
//! `clippy::float_arithmetic` lint). The reason is not precision but
//! reproducibility: `f64` results depend on FMA contraction, x87 excess
//! precision, SIMD vectorisation width, and the compiler's freedom to
//! reassociate — all of which vary by target and optimisation level. Two
//! machines running the same source can disagree on the last bit, and in a
//! system where a distance comparison selects an action, the last bit
//! decides which action fires.
//!
//! [`Fixed`] is an `i64` scaled by [`Fixed::SCALE`], giving exactly six
//! decimal places with integer semantics: exact addition, exact
//! comparison, and truncation that is specified rather than incidental.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Number of decimal places carried by [`Fixed`].
pub const DECIMALS: u32 = 6;

/// A fixed-point number with [`DECIMALS`] decimal places, backed by `i64`.
///
/// Ordering is the ordering of the underlying integer, so comparison is
/// exact and total — no `NaN`, no negative zero, no epsilon comparisons.
/// Serde reads a YAML integer as a whole number and a string as a decimal:
/// `900` is nine hundred, `"1.5"` is one and a half. It deliberately does
/// **not** round-trip the raw scaled representation, because a pack author
/// writing `temperature: 900` means 900, not 0.0009.
///
/// Canonical encoding uses [`Fixed::raw`] rather than serde, so stored
/// hashes are unaffected by this representation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fixed(i64);

impl Serialize for Fixed {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Fixed {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct FixedVisitor;

        impl<'de> Visitor<'de> for FixedVisitor {
            type Value = Fixed;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a whole number or a decimal string such as \"1.5\"")
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Fixed, E> {
                Fixed::from_int(value).map_err(|error| E::custom(error.to_string()))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Fixed, E> {
                let value = i64::try_from(value)
                    .map_err(|_| E::custom(format!("{value} is out of range")))?;
                Fixed::from_int(value).map_err(|error| E::custom(error.to_string()))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Fixed, E> {
                Fixed::parse(value).map_err(|error| E::custom(error.to_string()))
            }

            /// Recognised only in order to refuse it usefully.
            ///
            /// `{fixed: "44.0"}` is the tagged spelling of a fixed-point
            /// value, and it is correct in a general `Value` position where
            /// `44` could be an integer and `"44.0"` could be a symbol.
            /// Here the position is already typed as `Fixed`, so there is
            /// nothing to disambiguate and the tag is not part of the
            /// grammar.
            ///
            /// The default serde error for this says "invalid type: map",
            /// which is true and unhelpful: it tells an author what they
            /// wrote and not why the spelling they generalised from does
            /// not apply. An independent author hit exactly this and
            /// recovered by guessing correctly. The next one should not
            /// have to guess.
            ///
            /// This does **not** accept the map. Two spellings for one
            /// concept in a position that needs only one would widen the
            /// accepted surface without adding any expressive power, and
            /// raise a canonical-form question — which spelling should
            /// documentation and generated packs emit?
            ///
            /// **The decision stays falsifiable.** One
            /// author, a missing example in the brief, and a clean recovery
            /// is a documentation gap. If two or more independent authors
            /// still reach for the tagged form *after* the brief carries
            /// the example, that is evidence the distinction is
            /// semantically real and ergonomically poor — and accepting it
            /// everywhere becomes evidence-based rather than speculative.
            /// Until then the strictness is the cheaper bet, because a
            /// spelling can always be added and never removed.
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                _map: A,
            ) -> std::result::Result<Fixed, A::Error> {
                Err(de::Error::custom(concat!(
                    "this position is already typed as fixed-point: ",
                    "write 44 or \"44.0\". The tagged {fixed: \"44.0\"} form is used ",
                    "only in general value positions, where the kind must be told ",
                    "apart from an integer or a symbol",
                )))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Fixed, E> {
                Err(E::custom(format!(
                    "floating point is not permitted (got {value}); quote it as \"{value}\""
                )))
            }
        }

        deserializer.deserialize_any(FixedVisitor)
    }
}

impl Fixed {
    /// Scaling factor: `10^DECIMALS`.
    pub const SCALE: i64 = 1_000_000;

    /// Zero.
    pub const ZERO: Self = Self(0);

    /// One.
    pub const ONE: Self = Self(Self::SCALE);

    /// Largest representable value.
    pub const MAX: Self = Self(i64::MAX);

    /// Smallest representable value.
    pub const MIN: Self = Self(i64::MIN);

    /// Construct from a whole number.
    pub fn from_int(value: i64) -> Result<Self> {
        value
            .checked_mul(Self::SCALE)
            .map(Self)
            .ok_or(CoreError::FixedOverflow {
                operation: "from_int",
            })
    }

    /// Parse a decimal string such as `"1.5"` or `"-0.25"`.
    ///
    /// Rejects more than [`DECIMALS`] decimal places rather than rounding.
    /// Rounding would be a second place where a value's meaning depends on
    /// a policy nobody declared, and silently dropping precision from a
    /// model pack is exactly the kind of quiet change this project exists
    /// to prevent.
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = |reason: &'static str| CoreError::InvalidFixed {
            value: text.to_owned(),
            reason,
        };

        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };

        let (whole_text, fraction_text) = match rest.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (rest, ""),
        };

        if whole_text.is_empty() || !whole_text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("expected digits before the decimal point"));
        }
        if !fraction_text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("expected digits after the decimal point"));
        }
        if fraction_text.len() > DECIMALS as usize {
            return Err(invalid("more decimal places than Fixed can represent"));
        }

        let whole: i64 = whole_text
            .parse()
            .map_err(|_| invalid("integer part out of range"))?;

        let mut fraction: i64 = 0;
        for index in 0..DECIMALS as usize {
            let digit = fraction_text
                .as_bytes()
                .get(index)
                .map_or(0, |b| i64::from(b - b'0'));
            fraction = fraction * 10 + digit;
        }

        let scaled = whole
            .checked_mul(Self::SCALE)
            .and_then(|scaled| scaled.checked_add(fraction))
            .ok_or(CoreError::FixedOverflow { operation: "parse" })?;

        Ok(Self(if negative { -scaled } else { scaled }))
    }

    /// Construct directly from the scaled representation.
    ///
    /// `Fixed::from_raw(1_500_000)` is `1.5`.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The underlying scaled integer.
    ///
    /// This is the value that gets canonically encoded and hashed.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Truncate toward zero to a whole number.
    #[must_use]
    pub const fn to_int_trunc(self) -> i64 {
        self.0 / Self::SCALE
    }

    /// Checked addition.
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(CoreError::FixedOverflow { operation: "add" })
    }

    /// Checked subtraction.
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(CoreError::FixedOverflow { operation: "sub" })
    }

    /// Checked multiplication.
    ///
    /// Computed in `i128` so the intermediate product cannot overflow
    /// before rescaling. The result truncates toward zero.
    pub fn checked_mul(self, rhs: Self) -> Result<Self> {
        let product = i128::from(self.0) * i128::from(rhs.0);
        let scaled = product / i128::from(Self::SCALE);
        i64::try_from(scaled)
            .map(Self)
            .map_err(|_| CoreError::FixedOverflow { operation: "mul" })
    }

    /// Checked division. The result truncates toward zero.
    pub fn checked_div(self, rhs: Self) -> Result<Self> {
        if rhs.0 == 0 {
            return Err(CoreError::FixedDivideByZero);
        }
        let numerator = i128::from(self.0) * i128::from(Self::SCALE);
        let scaled = numerator / i128::from(rhs.0);
        i64::try_from(scaled)
            .map(Self)
            .map_err(|_| CoreError::FixedOverflow { operation: "div" })
    }

    /// Absolute value.
    pub fn checked_abs(self) -> Result<Self> {
        self.0
            .checked_abs()
            .map(Self)
            .ok_or(CoreError::FixedOverflow { operation: "abs" })
    }

    /// Negation.
    pub fn checked_neg(self) -> Result<Self> {
        self.0
            .checked_neg()
            .map(Self)
            .ok_or(CoreError::FixedOverflow { operation: "neg" })
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let scale = Self::SCALE.unsigned_abs();
        let whole = magnitude / scale;
        let fraction = magnitude % scale;
        write!(
            f,
            "{sign}{whole}.{fraction:0width$}",
            width = DECIMALS as usize
        )
    }
}

impl fmt::Debug for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed({self})")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn adds_exactly() {
        let a = Fixed::from_raw(100_000); // 0.1
        let b = Fixed::from_raw(200_000); // 0.2
                                          // The canonical floating point embarrassment: 0.1 + 0.2 != 0.3.
                                          // Here it is exact, which is the entire point of the type.
        assert_eq!(
            a.checked_add(b).expect("no overflow"),
            Fixed::from_raw(300_000)
        );
    }

    #[test]
    fn multiplies_with_rescaling() {
        let a = Fixed::from_int(3).expect("in range");
        let b = Fixed::from_raw(500_000); // 0.5
        assert_eq!(
            a.checked_mul(b).expect("no overflow"),
            Fixed::from_raw(1_500_000)
        );
    }

    #[test]
    fn divides_with_rescaling() {
        let a = Fixed::from_int(3).expect("in range");
        let b = Fixed::from_int(2).expect("in range");
        assert_eq!(
            a.checked_div(b).expect("no overflow"),
            Fixed::from_raw(1_500_000)
        );
    }

    #[test]
    fn reports_overflow_rather_than_wrapping() {
        assert!(Fixed::MAX.checked_add(Fixed::ONE).is_err());
        assert!(Fixed::from_int(i64::MAX).is_err());
    }

    #[test]
    fn reports_division_by_zero() {
        assert_eq!(
            Fixed::ONE.checked_div(Fixed::ZERO),
            Err(CoreError::FixedDivideByZero)
        );
    }

    #[test]
    fn truncates_toward_zero_in_both_directions() {
        let three = Fixed::from_int(3).expect("in range");
        let two = Fixed::from_int(2).expect("in range");
        let negative_three = three.checked_neg().expect("in range");
        assert_eq!(three.checked_div(two).expect("ok").to_int_trunc(), 1);
        assert_eq!(
            negative_three.checked_div(two).expect("ok").to_int_trunc(),
            -1
        );
    }

    #[test]
    fn renders_with_fixed_width_fraction() {
        assert_eq!(Fixed::from_raw(1_500_000).to_string(), "1.500000");
        assert_eq!(Fixed::from_raw(-1_500_000).to_string(), "-1.500000");
        assert_eq!(Fixed::ZERO.to_string(), "0.000000");
    }
}
