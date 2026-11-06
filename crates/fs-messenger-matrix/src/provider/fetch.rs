//! One sync: what happened since the cursor, as `RawMessage`s.
//!
//! Pattern: Template method: sync, close gaps, convert, retry what waits for a key, say where we are.
//! A failure anywhere fails the fetch, so the cursor stays and the next one replays; the core dedupes.

use std::time::Duration;

use fs_messenger_core::{FetchBatch, ProviderCursor};
use matrix_sdk::RoomState;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::config::SyncToken;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::api::client::filter::FilterDefinition;
use matrix_sdk::ruma::api::client::sync::sync_events::v3::Filter;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{OwnedRoomId, RoomId, UInt};
use matrix_sdk::store::{StateStoreDataKey, StateStoreDataValue};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::{MatrixCursor, RoomContext};

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
            .and_then(StateStoreDataValue::into_sync_token);
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
                let id = event.event_id().map(ToString::to_string);
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

    pub(super) fn room_context(&self, room_id: &RoomId) -> RoomContext {
        RoomContext {
            room_id: room_id.to_string(),
            name: self.client.get_room(room_id).and_then(|r| r.name()),
            own_user: self.account.user_id.clone(),
        }
    }
}
