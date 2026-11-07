//! One sync: what happened since the cursor, as `RawMessage`s.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). One
//! fetch, in order: sync, plan and read gaps, convert, retry what waits for a key, say where we
//! are. A failure anywhere fails the fetch, so the cursor stays and the next one replays; the core
//! dedupes.

use std::time::Duration;

use fs_messenger_core::{FetchBatch, ProviderCursor};
use matrix_sdk::RoomState;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::config::SyncToken;
use matrix_sdk::ruma::api::client::filter::FilterDefinition;
use matrix_sdk::ruma::api::client::sync::sync_events::v3::Filter;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{RoomId, UInt};
use matrix_sdk::store::{StateStoreDataKey, StateStoreDataValue};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::{EventAlias, Gap, MatrixCursor, RoomContext};

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
        self.plan_replay(&mut cursor, position.as_deref());

        let settings = self.sync_settings(position.clone());
        let response = self
            .account
            .bounded(self.client.sync_once(settings))
            .await?;
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
        let mut events = Vec::new();
        let limit = usize::try_from(self.account.timeline_limit).unwrap_or(usize::MAX);
        let mut cut = false;
        for (room_id, mut timeline) in joined.chain(left) {
            // `limit` is only a request. What is over it is read again through `/messages`, in pages,
            // by the replay of the next fetch: the cursor keeps its old `since` for that.
            if timeline.events.len() > limit {
                let over = timeline.events.len() - limit;
                timeline.events.drain(..over);
                cut = true;
            }
            let known = cursor.rooms.iter().any(|r| r == room_id.as_str());
            if let (true, Some(from)) = (!known || timeline.limited, timeline.prev_batch) {
                let to = if known { position.clone() } else { None };
                cursor.plan_gap(Gap::new(room_id.to_string(), from, to));
            }
            if !known {
                cursor.rooms.push(room_id.to_string());
            }
            events.push((room_id, timeline.events));
        }
        let (read, unfinished) = self.read_gaps(&mut cursor).await?;
        events.extend(read);

        let mut messages = Vec::new();
        for (room_id, list) in events {
            let context = self.room_context(&room_id);
            for event in list {
                let alias = event
                    .event_id()
                    .map(|id| EventAlias::new(room_id.as_str(), id));
                if alias.is_some_and(|alias| sent.contains(&alias)) {
                    continue;
                }
                let raw = self.convert(&room_id, &context, event, &mut cursor).await?;
                messages.extend(raw);
            }
        }
        drop(sent);

        messages.extend(self.retry_pending(&mut cursor).await?);
        if !cut {
            cursor.since = Some(response.next_batch);
        }
        Ok(FetchBatch {
            messages,
            more: unfinished || cut,
            next: Some(cursor.write()?),
        })
    }

    /// What the SDK has seen and the core has not: a crash between the two, or a core that lost its
    /// cursor. Read through `/messages`, per room.
    fn plan_replay(&self, cursor: &mut MatrixCursor, position: Option<&str>) {
        let Some(position) = position.filter(|p| cursor.since.as_deref() != Some(*p)) else {
            return;
        };
        for room in self.client.rooms() {
            if room.state() == RoomState::Invited {
                continue;
            }
            let id = room.room_id().to_string();
            let to = cursor.since.clone();
            cursor.plan_gap(Gap::new(id.clone(), position.to_owned(), to));
            if !cursor.rooms.contains(&id) {
                cursor.rooms.push(id);
            }
        }
    }

    pub(super) fn room_context(&self, room_id: &RoomId) -> RoomContext {
        RoomContext {
            room_id: room_id.to_string(),
            name: self.client.get_room(room_id).and_then(|r| r.name()),
            own_user: self.account.user_id.clone(),
        }
    }
}
