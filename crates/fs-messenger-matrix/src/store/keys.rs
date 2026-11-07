//! Keys of the journals: a record kind plus its parts, as one unambiguous text.
//!
//! Pattern: none, allowed by section 2 (small group of pure functions for one concept: the keys of the journal).
//! Pure functions. A key is a JSON array, so a state key or a room id from a server can hold any
//! character and still never run into the next part; a prefix is the same text without its end, so
//! removing "everything of this room" cannot touch another room.

use crate::error::StoreFail;

/// The key of a record.
pub(crate) fn key(parts: &[&str]) -> String {
    serde_json::to_string(parts).expect("strings always serialize")
}

/// What all keys below these parts start with.
pub(crate) fn prefix(parts: &[&str]) -> String {
    let mut text = key(parts);
    text.pop();
    text.push(',');
    text
}

/// The parts of a key.
pub(crate) fn parts(key: &str) -> Result<Vec<String>, StoreFail> {
    Ok(serde_json::from_str(key)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_cannot_run_together() {
        assert_ne!(key(&["a\",\"b"]), key(&["a", "b"]));
        assert_eq!(parts(&key(&["a\",\"b", "c"])).unwrap(), ["a\",\"b", "c"]);
    }

    #[test]
    fn a_prefix_stops_at_the_part() {
        let room = prefix(&["s", "!a:x"]);
        assert!(key(&["s", "!a:x", "t"]).starts_with(&room));
        assert!(!key(&["s", "!a:xy", "t"]).starts_with(&room));
    }
}
