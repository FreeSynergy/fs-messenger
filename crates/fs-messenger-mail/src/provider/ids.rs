//! Mail ids as the core may hold them, and back into a header.
//!
//! Pattern: Adapter (a reversible encoding). A `Message-ID` may legally hold `|` (RFC 5322 `atext`), and
//! the core refuses `|` and control characters in an alias, so such a mail and every reply to it would
//! be lost. `%`-encoding is injective: two different ids stay different, and the original comes back for
//! `In-Reply-To`. An id that was encoded for a control character never goes back into a header: a CRLF
//! there would be header injection.

use std::fmt::Write as _;

use fs_messenger_core::ExternalId;

/// The form the core holds. `%`, `|` and control characters are encoded, byte by byte.
pub(super) fn encode_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        if c == '%' || c == '|' || c.is_control() {
            let mut buffer = [0; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                let _ = write!(out, "%{byte:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The id as it was written. What this crate did not encode stays as it is.
pub(super) fn decode_id(held: &str) -> String {
    let bytes = held.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = (bytes[at] == b'%')
            .then(|| held.get(at + 1..at + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(byte) = hex {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The id for a mail header, if it is safe there.
pub(super) fn header_id(held: &str) -> Option<String> {
    Some(decode_id(held)).filter(|id| !id.contains(char::is_control))
}

/// An id as the core takes it, if it does at all: length is the limit encoding cannot help with.
pub(super) fn kept(scheme: &str, id: &str) -> Option<ExternalId> {
    Some(ExternalId::new(scheme, encode_id(id))).filter(ExternalId::is_well_formed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_round_trips_and_stays_injective() {
        for id in [
            "plain@x", "a|b@x", "a%7Cb@x", "100%@x", "%25|%@x", "x\0y", "a\r\nb", "é\u{85}",
        ] {
            let held = ExternalId::new("s", encode_id(id));
            assert!(held.is_well_formed(), "{held:?}");
            assert_eq!(decode_id(&held.id), id);
        }
        assert_ne!(encode_id("a|b@x"), encode_id("a%7Cb@x"));
    }

    #[test]
    fn a_control_character_never_goes_back_into_a_header() {
        assert_eq!(header_id(&encode_id("a|b@x")).as_deref(), Some("a|b@x"));
        assert_eq!(header_id(&encode_id("a\r\nBcc: evil@x")), None);
    }

    #[test]
    fn what_is_too_long_is_not_kept() {
        assert!(kept("s", "short").is_some());
        assert!(kept("s", &"x".repeat(5000)).is_none());
    }
}
