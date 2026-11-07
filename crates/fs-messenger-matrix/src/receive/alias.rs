//! [`EventAlias`]: the id under which the core knows a Matrix event.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). A
//! plain value with two public fields and no invariant to keep, so no Newtype. An event id alone is
//! not enough: the core replaces a placeholder by alias without comparing conversation or sender,
//! and a hostile homeserver can send into room B an event carrying the id of a placeholder from room
//! A. So the alias holds the room as well, and every place that builds, reads or looks up an alias
//! goes through this type.

use std::fmt::Write;

use fs_messenger_core::ExternalId;

/// Scheme of a Matrix event alias: what the provider issued, so what recognises a replay.
pub const SCHEME_EVENT: &str = "matrix";

const SEPARATOR: char = '/';

/// A room id and an event id, as one alias: `<room>/<event>`. In each part `%`, `/`, `|` and control
/// characters are written `%XX`, so the single unescaped `/` is the separator whatever a server puts
/// in an id, and the core, which refuses `|` and control characters in an id, keeps the alias.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventAlias {
    /// The room the event was sent into.
    pub room: String,
    /// The event's id.
    pub event: String,
}

fn escape(part: &str) -> String {
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

fn unescape(part: &str) -> Option<String> {
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

impl EventAlias {
    /// The alias of `event` in `room`.
    pub fn new(room: impl Into<String>, event: impl Into<String>) -> Self {
        Self {
            room: room.into(),
            event: event.into(),
        }
    }

    /// The text part of the alias, without the scheme.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}{SEPARATOR}{}", escape(&self.room), escape(&self.event))
    }

    /// The core's identifier.
    #[must_use]
    pub fn external(&self) -> ExternalId {
        ExternalId::new(SCHEME_EVENT, self.id())
    }

    /// Read the text part of an alias. `None` if it is not one this provider issues.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let (room, event) = id.split_once(SEPARATOR)?;
        if event.contains(SEPARATOR) {
            return None;
        }
        Some(Self {
            room: unescape(room)?,
            event: unescape(event)?,
        })
    }

    /// Read a core identifier of this scheme.
    #[must_use]
    pub fn from_external(id: &ExternalId) -> Option<Self> {
        (id.scheme == SCHEME_EVENT)
            .then(|| Self::parse(&id.id))
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_whatever_the_ids_hold() {
        for (room, event) in [
            ("!r:x", "$e"),
            ("!a/b|:x", "$c/d|"),
            ("!%7C:x", "$%25"),
            ("", ""),
            ("!r:x/", "/$e"),
        ] {
            let alias = EventAlias::new(room, event);
            assert_eq!(EventAlias::parse(&alias.id()), Some(alias.clone()));
            assert_eq!(EventAlias::from_external(&alias.external()), Some(alias));
        }
    }

    #[test]
    fn the_core_keeps_it() {
        // The core refuses an id with `|` or a control character: its store keys are built with `|`.
        for (room, event) in [("!r:x", "$e"), ("!a|b\n:x", "$c|d\r")] {
            assert!(EventAlias::new(room, event).external().is_well_formed());
        }
    }

    #[test]
    fn different_pairs_never_share_an_alias() {
        let a = EventAlias::new("!a/b", "$c").id();
        let b = EventAlias::new("!a", "b/$c").id();
        assert_ne!(a, b);
    }

    #[test]
    fn a_foreign_or_old_alias_is_not_read() {
        assert_eq!(EventAlias::parse("$e"), None);
        assert_eq!(EventAlias::parse("!r/$e/more"), None);
        assert_eq!(EventAlias::parse("!r/%zz"), None);
        assert_eq!(
            EventAlias::parse("!r/%41"),
            None,
            "not the spelling we write"
        );
        assert_eq!(EventAlias::parse("!r/$e%"), None);
        let other = ExternalId::new("matrix-room", "!r/$e");
        assert_eq!(EventAlias::from_external(&other), None);
    }
}
