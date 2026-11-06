//! Mail ids as the core may hold them.
//!
//! Pattern: Adapter (a reversible encoding). A `Message-ID` may legally hold `|` (RFC 5322 `atext`), and
//! the core refuses it in an alias, so such a mail and every reply to it would be lost. `%`-encoding is
//! injective: two different ids stay different, and the original comes back for `In-Reply-To`.

/// The form the core holds. `%`, `|` and NUL are encoded.
pub(super) fn encode_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        match c {
            '%' => out.push_str("%25"),
            '|' => out.push_str("%7C"),
            '\0' => out.push_str("%00"),
            other => out.push(other),
        }
    }
    out
}

/// The id as it was written. What this crate did not encode stays as it is.
pub(super) fn decode_id(held: &str) -> String {
    held.replace("%7C", "|")
        .replace("%00", "\0")
        .replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_round_trips_and_stays_injective() {
        for id in ["plain@x", "a|b@x", "a%7Cb@x", "100%@x", "%25|%@x", "x\0y"] {
            let held = encode_id(id);
            assert!(!held.contains(['|', '\0']), "{held}");
            assert_eq!(decode_id(&held), id);
        }
        assert_ne!(encode_id("a|b@x"), encode_id("a%7Cb@x"));
    }
}
