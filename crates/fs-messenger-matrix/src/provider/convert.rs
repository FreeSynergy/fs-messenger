//! One event to one `RawMessage`, and another try at what had no key.
//!
//! Pattern: Adapter over what the SDK made of an event. The mark comes from how the SDK came by the
//! text: `Decrypted` is `megolm`, `PlainText` is `none`, anything else claims nothing.

use fs_messenger_core::{Encryption, RawMessage};
use matrix_sdk::deserialized_responses::{AlgorithmInfo, TimelineEvent, TimelineEventKind};
use matrix_sdk::ruma::api::client::room::get_room_event;
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedEventId, RoomId};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::{
    MatrixCursor, Pending, RoomContext, late_message, message, parse, placeholder,
};

const ROOM_MESSAGE: &str = "m.room.message";
const ROOM_ENCRYPTED: &str = "m.room.encrypted";

fn megolm_if_decrypted(event: &TimelineEvent) -> Encryption {
    match event.encryption_info().map(|i| &i.algorithm_info) {
        Some(AlgorithmInfo::MegolmV1AesSha2 { .. }) => Encryption::Megolm,
        // Decrypted by something this provider does not know: claims nothing.
        _ => Encryption::Unknown,
    }
}

fn wait_for_key(context: &RoomContext, json: &str, cursor: &mut MatrixCursor) -> RawMessage {
    let parsed = parse(json).unwrap_or_default();
    cursor.wait_for(Pending {
        room: context.room_id.clone(),
        event: parsed.event_id.clone(),
        raw: json.to_owned(),
    });
    placeholder(context, &parsed, json)
}

impl MatrixProvider {
    pub(super) async fn convert(
        &self,
        room_id: &RoomId,
        context: &RoomContext,
        event: TimelineEvent,
        cursor: &mut MatrixCursor,
    ) -> Result<Option<RawMessage>> {
        let encryption = megolm_if_decrypted(&event);
        match &event.kind {
            TimelineEventKind::Decrypted(decrypted) => {
                let Some(parsed) = parse(decrypted.event.json().get()) else {
                    return Ok(None);
                };
                if parsed.event_type != ROOM_MESSAGE {
                    return Ok(None);
                }
                let original = self.ciphertext_of(room_id, &parsed.event_id).await?;
                Ok(message(context, &parsed, encryption, Some(&original)))
            }
            TimelineEventKind::PlainText { event: raw } => {
                let json = raw.json().get();
                let Some(parsed) = parse(json) else {
                    return Ok(None);
                };
                match parsed.event_type.as_str() {
                    ROOM_MESSAGE => Ok(message(context, &parsed, Encryption::None, Some(json))),
                    // Encrypted, and the SDK did not even try: treated like one it could not open.
                    ROOM_ENCRYPTED => Ok(Some(wait_for_key(context, json, cursor))),
                    _ => Ok(None),
                }
            }
            TimelineEventKind::UnableToDecrypt { event: raw, .. } => {
                let json = raw.json().get();
                Ok(parse(json).map(|_| wait_for_key(context, json, cursor)))
            }
        }
    }

    /// The event as the server holds it. The SDK hands over only the cleartext of what it decrypted,
    /// so the original of ADR 0030 takes one more request. A failure here fails the fetch.
    async fn ciphertext_of(&self, room_id: &RoomId, event_id: &str) -> Result<String> {
        let id: OwnedEventId = event_id
            .try_into()
            .map_err(|_| MatrixError::Sdk(format!("bad event id {event_id}")))?;
        let request = get_room_event::v3::Request::new(room_id.to_owned(), id);
        let response = self.client.send(request).await?;
        Ok(response.event.json().get().to_owned())
    }

    /// Try again what had no key, without the network. What opens now leaves the list.
    pub(super) async fn retry_pending(&self, cursor: &mut MatrixCursor) -> Result<Vec<RawMessage>> {
        let mut messages = Vec::new();
        let mut still = Vec::new();
        for pending in std::mem::take(&mut cursor.pending) {
            let room_id = <&RoomId>::try_from(pending.room.as_str())
                .map_err(|e| MatrixError::Cursor(e.to_string()))?;
            let Some(room) = self.client.get_room(room_id) else {
                still.push(pending);
                continue;
            };
            let raw: Raw<OriginalSyncRoomEncryptedEvent> =
                Raw::from_json_string(pending.raw.clone()).map_err(crate::StoreFail::from)?;
            let event = room.decrypt_event(&raw, None).await?;
            if !matches!(event.kind, TimelineEventKind::Decrypted(_)) {
                still.push(pending);
                continue;
            }
            let encryption = megolm_if_decrypted(&event);
            let context = self.room_context(room_id);
            let json = event.raw().json().get().to_owned();
            if let Some(parsed) = parse(&json).filter(|p| p.event_type == ROOM_MESSAGE) {
                messages.extend(late_message(&context, &parsed, encryption, &pending.raw));
            }
        }
        cursor.pending = still;
        Ok(messages)
    }
}
