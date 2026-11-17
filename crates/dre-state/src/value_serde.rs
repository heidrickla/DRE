//! Natural YAML representation for [`Value`].
//!
//! The derived enum representation would force a pack author to write
//! `temperature: {integer: 750}`. Model packs are the project's user
//! interface, so values are read from plain scalars instead:
//!
//! ```yaml
//! bearing_fault: true          # Boolean
//! temperature:   750           # Integer
//! grade:         grade-a       # Symbol
//! efficiency:    {fixed: 0.5}  # Fixed
//! paired_with:   {ref: pump-1} # Reference
//! ```
//!
//! The two map forms exist because the scalar forms are ambiguous:
//! `Symbol` and `Reference` are both identifiers, and a bare `0.5` cannot
//! be told from an integer without deciding what decimals mean.
//!
//! **Floats are rejected with a message pointing at `{fixed: …}`.** A
//! silently accepted `f64` in a model pack would be a hole straight
//! through the determinism guarantee, so the error is worth more than the
//! convenience.

use std::fmt;

use dre_core::fixed::Fixed;
use dre_core::id::Id;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Value;

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a boolean, integer, identifier, {fixed: N} or {ref: ID}")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Boolean(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Integer(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        i64::try_from(value)
            .map(Value::Integer)
            .map_err(|_| E::custom(format!("integer {value} is out of range for i64")))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Err(E::custom(format!(
            "floating point is not permitted in model packs (got {value}); \
             write a fixed-point value as {{fixed: \"{value}\"}}"
        )))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Id::new(value)
            .map(Value::Symbol)
            .map_err(|error| E::custom(error.to_string()))
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Value, M::Error> {
        let Some(key) = map.next_key::<String>()? else {
            return Err(de::Error::custom("empty mapping is not a value"));
        };

        let value = match key.as_str() {
            // `Fixed`'s own deserializer accepts a whole number or a
            // quoted decimal, and refuses floats.
            "fixed" => Value::Fixed(map.next_value::<Fixed>()?),
            "ref" => {
                let raw: String = map.next_value()?;
                Value::Reference(Id::new(raw).map_err(|e| de::Error::custom(e.to_string()))?)
            }
            other => {
                return Err(de::Error::custom(format!(
                    "unknown value form {other:?}, expected `fixed` or `ref`"
                )));
            }
        };

        if map.next_key::<String>()?.is_some() {
            return Err(de::Error::custom("a value mapping takes exactly one key"));
        }

        Ok(value)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        match self {
            Self::Boolean(value) => serializer.serialize_bool(*value),
            Self::Integer(value) => serializer.serialize_i64(*value),
            Self::Symbol(value) => serializer.serialize_str(value.as_str()),
            Self::Fixed(value) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("fixed", &value.to_string())?;
                map.end()
            }
            Self::Reference(value) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("ref", value.as_str())?;
                map.end()
            }
        }
    }
}
