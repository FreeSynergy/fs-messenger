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
/// 2^53.
const MAX_SAFE: f64 = 9_007_199_254_740_992.0;

/// Why these arguments cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArgumentsError {
    /// Not JSON, or an object has a key twice.
    #[error("not usable as arguments: {0}")]
    Invalid(String),
    /// Nested too deep.
    #[error("nested deeper than {MAX_DEPTH} levels")]
    TooDeep,
    /// A number at or above 2^53, or one a float cannot hold exactly as written. Send it as a string.
    #[error("the number {0} has no one meaning; send it as a string")]
    NumberOutOfRange(String),
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

/// Over 2^53 a JSON number has no one meaning: 2^64 and 2^64 + 1 parse alike here, and not in the tool.
fn usable_number(n: &serde_json::Number) -> Result<(), ArgumentsError> {
    let in_range = n
        .as_f64()
        .is_some_and(|f| f.is_finite() && f.abs() < MAX_SAFE);
    if in_range {
        Ok(())
    } else {
        Err(ArgumentsError::NumberOutOfRange(n.to_string()))
    }
}

/// Digits and power of ten of a decimal number, in lowest terms; `None` if it is zero.
fn decimal(text: &str) -> Option<(String, i64)> {
    let plain = text.trim_start_matches('-');
    let (mantissa, exponent) = match plain.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i64>().unwrap_or(i64::MAX / 2)),
        None => (plain, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let scale = i64::try_from(fraction.len()).unwrap_or(0);
    let start = digits.trim_start_matches('0');
    let end = start.trim_end_matches('0');
    if end.is_empty() {
        return None;
    }
    let dropped = i64::try_from(start.len() - end.len()).unwrap_or(0);
    Some((end.to_string(), exponent - scale + dropped))
}

/// Refuse a number in the text that a float cannot hold exactly as written: what is shown and hashed
/// would be other than what a tool reading the text gets. Strings are skipped.
fn numbers_round_trip(text: &str) -> Result<(), ArgumentsError> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = at;
                while at < bytes.len()
                    && matches!(bytes[at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    at += 1;
                }
                let number = &text[start..at];
                let held = number.parse::<f64>().map(|f| format!("{f:e}"));
                if !held.is_ok_and(|held| decimal(&held) == decimal(number)) {
                    return Err(ArgumentsError::NumberOutOfRange(number.to_string()));
                }
            }
            _ => at += 1,
        }
    }
    Ok(())
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
        Value::Number(n) => {
            usable_number(n)?;
            out.push_str(&n.to_string());
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
        numbers_round_trip(text)?;
        // The parser's own depth limit comes first; ours is lower.
        let Unique(value) =
            serde_json::from_str(text).map_err(|e| ArgumentsError::Invalid(e.to_string()))?;
        Self::from_value(&value)
    }

    /// The canonical form; `None` without input. **A tool layer runs this and nothing else**: not the
    /// text it was given. Only this is what the digest covers and the person was shown.
    pub fn canonical(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Whether there is input.
    pub fn has_input(&self) -> bool {
        self.0.is_some()
    }

    /// What a person reads: the whole canonical form, with what could hide or reorder text escaped.
    pub fn detail(&self) -> String {
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

/// Bidirectional controls, zero-width, blank-looking and other format characters, variation selectors,
/// line separators, tag characters.
pub(crate) fn hides_text(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180F
            | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0x2800 | 0x3164
            | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF9..=0xFFFB | 0x1D173..=0x1D17A
            | 0xE0000..=0xE007F | 0xE0100..=0xE01EF)
}
