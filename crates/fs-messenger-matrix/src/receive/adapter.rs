//! Building the `RawMessage`: aliases, threading, the encryption mark, the original.
//!
//! Pattern: Adapter. The mark says what really happened to this event, never what the room promises:
//! a plain event in an encrypted room is `none`, an event nobody could decrypt is `unknown`.

use fs_messenger_core::{Encryption, ExternalId, Kind, RawMessage};
use fs_store::Blob;

use super::event::ParsedEvent;

/// Scheme of a Matrix event id: what the provider issued, so what recognises a replay.
pub const SCHEME_EVENT: &str = "matrix";
/// Scheme of a room id: the conversation.
pub const SCHEME_ROOM: &str = "matrix-room";
/// Scheme of the text of an event whose key arrived after it was stored; see `DESIGN.md`.
pub const SCHEME_LATE: &str = "matrix-decrypted";
/// The body of a message that could not be decrypted. The original is kept with it.
pub const UNDECRYPTED_MARK: &str =
    "[This message could not be decrypted: its key has not arrived.]";

const ORIGINAL_TYPE: &str = "application/json";

/// The room an event belongs to, and whose view it is.
#[derive(Clone, Debug)]
pub(crate) struct RoomContext {
    pub(crate) room_id: String,
    pub(crate) name: Option<String>,
    pub(crate) own_user: String,
}

fn event_alias(id: &str) -> ExternalId {
    ExternalId::new(SCHEME_EVENT, id)
}

fn build(
    room: &RoomContext,
    event: &ParsedEvent,
    alias: ExternalId,
    body: String,
    encryption: Encryption,
    original: Option<&str>,
) -> RawMessage {
    let conversation = ExternalId::new(SCHEME_ROOM, &room.room_id);
    let mut raw = RawMessage::new(alias, conversation, &event.sender, body);
    raw.conversation_name.clone_from(&room.name);
    raw.kind = Kind::Chat;
    raw.encryption = encryption;
    // A room has no list of recipients in an event. The room is who it went to, so that answering
    // one's own message goes back there.
    raw.to = vec![room.room_id.clone()];
    raw.published = event.published;
    raw.in_reply_to = event.in_reply_to.as_deref().map(event_alias);
    raw.original = original.map(|json| Blob {
        media_type: ORIGINAL_TYPE.to_owned(),
        bytes: json.as_bytes().to_vec(),
    });
    raw.outgoing = event.sender == room.own_user;
    raw
}

/// A readable message. `None` if the event holds no text.
pub(crate) fn message(
    room: &RoomContext,
    event: &ParsedEvent,
    encryption: Encryption,
    original: Option<&str>,
) -> Option<RawMessage> {
    let body = event.body.clone()?;
    let alias = event_alias(&event.event_id);
    Some(build(room, event, alias, body, encryption, original))
}

/// A message that could not be decrypted: visibly so, not marked as protected or as plain.
pub(crate) fn placeholder(room: &RoomContext, event: &ParsedEvent, original: &str) -> RawMessage {
    let alias = event_alias(&event.event_id);
    let body = UNDECRYPTED_MARK.to_owned();
    build(
        room,
        event,
        alias,
        body,
        Encryption::Unknown,
        Some(original),
    )
}

/// The text of an event stored earlier as a placeholder. The core cannot replace a message, so this
/// is a new one that answers the placeholder (`DESIGN.md`, "Wanted from core").
pub(crate) fn late_message(
    room: &RoomContext,
    event: &ParsedEvent,
    encryption: Encryption,
    original: &str,
) -> Option<RawMessage> {
    let body = event.body.clone()?;
    let alias = ExternalId::new(SCHEME_LATE, &event.event_id);
    let mut raw = build(room, event, alias, body, encryption, Some(original));
    raw.in_reply_to = Some(event_alias(&event.event_id));
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> RoomContext {
        RoomContext {
            room_id: "!r:x".into(),
            name: Some("Room".into()),
            own_user: "@me:x".into(),
        }
    }

    fn event(sender: &str) -> ParsedEvent {
        ParsedEvent {
            event_type: "m.room.message".into(),
            event_id: "$e".into(),
            sender: sender.into(),
            published: None,
            body: Some("text".into()),
            in_reply_to: Some("$p".into()),
        }
    }

    #[test]
    fn the_event_id_is_the_alias_and_the_room_the_conversation() {
        let raw = message(&room(), &event("@a:x"), Encryption::None, Some("{}")).unwrap();
        assert_eq!(raw.aliases, [ExternalId::new(SCHEME_EVENT, "$e")]);
        assert_eq!(raw.conversation, ExternalId::new(SCHEME_ROOM, "!r:x"));
        assert_eq!(raw.in_reply_to, Some(ExternalId::new(SCHEME_EVENT, "$p")));
        assert_eq!(raw.original.unwrap().bytes, b"{}");
        assert!(!raw.outgoing);
    }

    #[test]
    fn one_s_own_event_is_outgoing() {
        let raw = message(&room(), &event("@me:x"), Encryption::Megolm, None).unwrap();
        assert!(raw.outgoing);
    }

    #[test]
    fn a_placeholder_claims_nothing() {
        let raw = placeholder(&room(), &event("@a:x"), "{}");
        assert_eq!(raw.encryption, Encryption::Unknown);
        assert!(!raw.encryption.is_end_to_end());
        assert_eq!(raw.body_text, UNDECRYPTED_MARK);
    }

    #[test]
    fn late_text_has_its_own_alias_and_answers_the_placeholder() {
        let raw = late_message(&room(), &event("@a:x"), Encryption::Megolm, "{}").unwrap();
        assert_eq!(raw.aliases, [ExternalId::new(SCHEME_LATE, "$e")]);
        assert_eq!(raw.in_reply_to, Some(ExternalId::new(SCHEME_EVENT, "$e")));
    }
}
