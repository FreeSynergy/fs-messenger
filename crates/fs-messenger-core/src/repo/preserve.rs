//! Writing a record back without losing what this version could not read.
//!
//! Pattern: Memento: reading keeps the raw form. On writing, a value that decoding changed (a newer
//! program's enum value read as `unknown`) or dropped (a field unknown here) comes back, but only
//! where this version left it untouched: a real change, also one to `unknown`, always wins.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::Result;

/// Put back into `new` what decoding `raw` as `T` lost.
pub(super) fn keep_unknown<T: Serialize + DeserializeOwned>(
    new: &mut Value,
    raw: Option<&Value>,
) -> Result<()> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let decoded: T = serde_json::from_value(raw.clone())?;
    restore(new, raw, &serde_json::to_value(decoded)?);
    Ok(())
}

fn restore(new: &mut Value, raw: &Value, base: &Value) {
    match (new, raw, base) {
        (Value::Object(new), Value::Object(raw), Value::Object(base)) => {
            for (key, raw_value) in raw {
                match (new.get_mut(key), base.get(key)) {
                    (Some(n), Some(b)) => restore(n, raw_value, b),
                    (None, None) => {
                        new.insert(key.clone(), raw_value.clone());
                    }
                    _ => {}
                }
            }
        }
        (Value::Array(new), Value::Array(raw), Value::Array(base)) => {
            let by_id = |list: &[Value], id: &Value| {
                list.iter().find(|item| item.get("id") == Some(id)).cloned()
            };
            for item in new {
                let Some(id) = item.get("id").cloned() else {
                    continue;
                };
                if let (Some(r), Some(b)) = (by_id(raw, &id), by_id(base, &id)) {
                    restore(item, &r, &b);
                }
            }
        }
        (new, raw, base) if *new == *base && raw != base => *new = raw.clone(),
        _ => {}
    }
}
