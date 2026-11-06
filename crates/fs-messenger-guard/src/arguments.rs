//! [`Arguments`]: what a tool call is asked to do, in one canonical form.
//!
//! Pattern: Newtype as proof: an `Arguments` exists only in canonical form, within the limit and without
//! a duplicate key. Two programs can read `{"path":"/a","path":"/"}` differently, so what a person saw
//! would not be what runs. Canonical form (ours, not RFC 8785): object keys sorted by bytes at every
//! level, no white space, strings and numbers as `serde_json` writes them. So `1` and `1.0` differ,
//! which at worst asks again. Keys are sorted here on purpose: whether `serde_json` keeps them sorted
//! depends on a feature any other crate in a build can turn on.

use std::fmt::Write as _;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

/// Largest canonical form. Over it a call cannot be asked about at all: a person cannot judge what
/// would have to be cut off, and the detail shown is never cut off. Provisional.
pub const MAX_CANONICAL_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 64;

/// Why these arguments cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArgumentsError {
    /// Not JSON, or an object has a key twice.
    #[error("not usable as arguments: {0}")]
    Invalid(String),
    /// Nested too deep.
    #[error("nested deeper than {MAX_DEPTH} levels")]
    TooDeep,
    /// Over the limit.
    #[error("canonical form of {0} bytes is over the limit of {MAX_CANONICAL_BYTES}")]
    TooLarge(usize),
}

/// The arguments of a call: none yet, or JSON in canonical form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arguments(Option<String>);

/// A JSON value that refuses a duplicate key while being read.
struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(UniqueVisitor).map(Unique)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = vec![];
        while let Some(Unique(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = serde_json::Map::new();
        while let Some((key, Unique(value))) = map.next_entry::<String, Unique>()? {
            if object.insert(key.clone(), value).is_some() {
                return Err(serde::de::Error::custom(format!("duplicate key {key:?}")));
            }
        }
        Ok(Value::Object(object))
    }
}

fn canonical(value: &Value, depth: usize, out: &mut String) -> Result<(), ArgumentsError> {
    if depth > MAX_DEPTH {
        return Err(ArgumentsError::TooDeep);
    }
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (at, key) in keys.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                canonical(&map[key], depth + 1, out)?;
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                canonical(item, depth + 1, out)?;
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
    if out.len() > MAX_CANONICAL_BYTES {
        return Err(ArgumentsError::TooLarge(out.len()));
    }
    Ok(())
}

impl Arguments {
    /// No input yet. Risk `unknown`, `allowOnce` only, whatever the capability.
    pub fn none() -> Self {
        Self(None)
    }

    /// From parsed JSON. `null` is no input. A `Value` has no duplicate keys left to find, so prefer
    /// [`Arguments::from_text`] for anything that came as text.
    pub fn from_value(value: &Value) -> Result<Self, ArgumentsError> {
        if value.is_null() {
            return Ok(Self::none());
        }
        let mut out = String::new();
        canonical(value, 0, &mut out)?;
        Ok(Self(Some(out)))
    }

    /// From JSON text, refusing a duplicate key at any level.
    pub fn from_text(text: &str) -> Result<Self, ArgumentsError> {
        // The parser's own depth limit comes first; ours is lower.
        let Unique(value) =
            serde_json::from_str(text).map_err(|e| ArgumentsError::Invalid(e.to_string()))?;
        Self::from_value(&value)
    }

    /// The canonical form; `None` without input.
    pub fn canonical(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Whether there is input.
    pub fn has_input(&self) -> bool {
        self.0.is_some()
    }

    /// What a person reads: the whole canonical form, with what could hide or reorder text escaped.
    pub(crate) fn detail(&self) -> String {
        let Some(text) = self.0.as_deref() else {
            return "(no input)".to_string();
        };
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if hides_text(c) {
                // Infallible on a String.
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// Bidirectional controls, zero-width and other format characters, line separators, tag characters.
pub(crate) fn hides_text(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F
            | 0xFEFF | 0xFFF9..=0xFFFB | 0xE0000..=0xE007F)
}
