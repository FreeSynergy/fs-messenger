//! One sync: what happened since the cursor, as `RawMessage`s.
//!
//! Pattern: Template method: sync, close gaps, convert, retry what waits for a key, say where we are.
//! A failure anywhere fails the fetch, so the cursor stays and the next one replays; the core dedupes.

use std::time::Duration;

use fs_messenger_core::{Encryption, FetchBatch, ProviderCursor, RawMessage};
use matrix_sdk::RoomState;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::config::SyncToken;
use matrix_sdk::deserialized_responses::{AlgorithmInfo, TimelineEvent, TimelineEventKind};
use matrix_sdk::ruma::api::client::filter::FilterDefinition;
use matrix_sdk::ruma::api::client::room::get_room_event;
use matrix_sdk::ruma::api::client::sync::sync_events::v3::Filter;
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, RoomId, UInt};
use matrix_sdk::store::StateStoreDataKey;

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

impl MatrixProvider {
    fn sync_settings(&self, since: Option<String>) -> SyncSettings {
        let mut filter = FilterDefinition::default();
        filter.room.timeline.limit = Some(UInt::from(self.account.timeline_limit));
        let token = since.map_or(SyncToken::NoToken, SyncToken::Specific);
        SyncSettings::new()
            .token(token)
            .timeout(Duration::ZERO)
            .set_presence(PresenceState::Offline)
            .filter(Filter::FilterDefinition(filter))
    }

    pub(super) async fn fetch_batch(&self, since: Option<&ProviderCursor>) -> Result<FetchBatch> {
        let mut cursor = MatrixCursor::read(since)?;
        // Held for the whole fetch: see `echo.rs`.
        let sent = self.sent.lock().await;
        // The SDK's own position, not the core's: the SDK ignores a response it has seen, so asking
        // from an older cursor would answer nothing and lose what a replay is for.
        let position = self
            .client
            .state_store()
            .get_kv_data(StateStoreDataKey::SyncToken)
            .await
            .map_err(|e| MatrixError::Sdk(e.to_string()))?
            .and_then(|v| v.into_sync_token());
        let mut events = self.replay_since(&cursor, position.as_deref()).await?;

        let settings = self.sync_settings(position.clone());
        let response = self.client.sync_once(settings).await?;
        let joined = response
            .rooms
            .joined
            .into_iter()
            .map(|(r, u)| (r, u.timeline));
        let left = response
            .rooms
            .left
            .into_iter()
            .map(|(r, u)| (r, u.timeline));
        for (room_id, timeline) in joined.chain(left) {
            let known = cursor.rooms.iter().any(|r| r == room_id.as_str());
            if !known || timeline.limited {
                let to = if known { position.as_deref() } else { None };
                let gap = self
                    .missing_events(&room_id, timeline.prev_batch, to)
                    .await?;
                events.push((room_id.clone(), gap));
            }
            events.push((room_id.clone(), timeline.events));
            if !known {
                cursor.rooms.push(room_id.to_string());
            }
        }

        let mut messages = Vec::new();
        for (room_id, list) in events {
            let context = self.room_context(&room_id);
            for event in list {
                let id = event.event_id().map(|e| e.to_string());
                if id.is_some_and(|id| sent.contains(&id)) {
                    continue;
                }
                let raw = self.convert(&room_id, &context, event, &mut cursor).await?;
                messages.extend(raw);
            }
        }
        drop(sent);

        messages.extend(self.retry_pending(&mut cursor).await?);
        cursor.since = Some(response.next_batch);
        Ok(FetchBatch {
            messages,
            next: Some(cursor.write()?),
            more: false,
        })
    }

    /// What the SDK has seen and the core has not: a crash between the two, or a core that lost its
    /// cursor. Comes from `/messages`, per room.
    async fn replay_since(
        &self,
        cursor: &MatrixCursor,
        position: Option<&str>,
    ) -> Result<Vec<(OwnedRoomId, Vec<TimelineEvent>)>> {
        let Some(position) = position.filter(|p| cursor.since.as_deref() != Some(*p)) else {
            return Ok(vec![]);
        };
        let mut events = Vec::new();
        for room in self.client.rooms() {
            if room.state() == RoomState::Invited {
                continue;
            }
            let room_id = room.room_id();
            let from = Some(position.to_owned());
            let gap = self
                .missing_events(room_id, from, cursor.since.as_deref())
                .await?;
            events.push((room_id.to_owned(), gap));
        }
        Ok(events)
    }

    fn room_context(&self, room_id: &RoomId) -> RoomContext {
        RoomContext {
            room_id: room_id.to_string(),
            name: self.client.get_room(room_id).and_then(|r| r.name()),
            own_user: self.account.user_id.clone(),
        }
    }

    async fn convert(
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
                    ROOM_ENCRYPTED => Ok(Some(self.wait_for_key(context, json, cursor))),
                    _ => Ok(None),
                }
            }
            TimelineEventKind::UnableToDecrypt { event: raw, .. } => {
                let json = raw.json().get();
                Ok(parse(json).map(|_| self.wait_for_key(context, json, cursor)))
            }
        }
    }

    fn wait_for_key(
        &self,
        context: &RoomContext,
        json: &str,
        cursor: &mut MatrixCursor,
    ) -> RawMessage {
        let parsed = parse(json).unwrap_or_default();
        cursor.wait_for(Pending {
            room: context.room_id.clone(),
            event: parsed.event_id.clone(),
            raw: json.to_owned(),
        });
        placeholder(context, &parsed, json)
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
    async fn retry_pending(&self, cursor: &mut MatrixCursor) -> Result<Vec<RawMessage>> {
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
