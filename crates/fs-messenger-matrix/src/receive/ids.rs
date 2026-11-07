//! One encoding for an id of the server inside an id of the core.
//!
//! Pattern: none, allowed by section 2 (small group of pure functions for one concept: the encoding).
//! The core refuses `|` and control characters in an `ExternalId`; Matrix allows them in a room id
//! (the specification allows any character but NUL in its local part) and an event id. So whatever a
//! server puts in an id, `%`, `/`, `|` and control characters are written `%XX`: one spelling per
//! text, read back without loss. The alias of an event and the conversation of a room use this and
//! nothing else.

use std::fmt::Write;

use fs_messenger_core::ExternalId;

use super::adapter::SCHEME_ROOM;

/// Between the parts of an alias; always escaped inside a part.
pub(crate) const SEPARATOR: char = '/';

pub(crate) fn escape(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for c in part.chars() {
        if matches!(c, '%' | SEPARATOR | '|') || c.is_control() {
            let mut buf = [0; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{byte:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn unescape(part: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(part.len());
    let mut rest = part.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    // Only what `escape` writes: another spelling of the same pair would be a second alias for it.
    (escape(&text) == part).then_some(text)
}

/// The core's conversation id for a room.
pub(crate) fn conversation_of(room_id: &str) -> ExternalId {
    ExternalId::new(SCHEME_ROOM, escape(room_id))
}

/// The room a conversation id of this scheme stands for. `None` for another scheme or a spelling
/// this crate does not write.
pub(crate) fn room_of(conversation: &ExternalId) -> Option<String> {
    (conversation.scheme == SCHEME_ROOM)
        .then(|| unescape(&conversation.id))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_id_survives_the_core_whatever_it_holds() {
        for room in [
            "!r:x",
            "!a|b:x",
            "!a\nb\u{1}\u{7f}c:x",
            "!a/b%7C:x",
            "!ü€:x",
        ] {
            let id = conversation_of(room);
            assert!(id.is_well_formed(), "{room:?}");
            assert_eq!(room_of(&id).as_deref(), Some(room));
        }
    }

    #[test]
    fn two_rooms_never_share_a_conversation() {
        assert_ne!(conversation_of("!a|b:x"), conversation_of("!a%7Cb:x"));
    }

    #[test]
    fn only_what_is_written_here_is_read_back() {
        assert_eq!(room_of(&ExternalId::new("matrix", "!r:x")), None);
        assert_eq!(room_of(&ExternalId::new(SCHEME_ROOM, "!a%7cb")), None);
        assert_eq!(room_of(&ExternalId::new(SCHEME_ROOM, "!a%zz")), None);
    }
}
