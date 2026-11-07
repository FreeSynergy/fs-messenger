//! One sync: what happened since the cursor, as `RawMessage`s.
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder): `fetch` of the `Provider` trait over the SDK's `/sync`. One
//! fetch, in order: sync, plan and read gaps, convert, retry what waits for a key, say where we
//! are. A failure anywhere fails the fetch, so the cursor stays and the next one replays; the core
//! dedupes. Which rooms a replay reads again is decided by the Specification in `replay.rs`.

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
use super::replay::{reads_to_start, worth_replaying};
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
        let next_batch = response.next_batch.clone();
        let had_gaps = !cursor.gaps.is_empty();
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
            // `limit` is only a request. What is over it is read again through `/messages`, as a gap
            // from the end of this answer down to where we were; the cursor moves on all the same.
            let known = cursor.rooms.iter().any(|r| r == room_id.as_str());
            let to = if known { position.clone() } else { None };
            if timeline.events.len() > limit {
                let over = timeline.events.len() - limit;
                timeline.events.drain(..over);
                cut = true;
                cursor.plan_gap(Gap::new(room_id.to_string(), next_batch.clone(), to));
            } else if let (true, Some(from)) = (!known || timeline.limited, timeline.prev_batch) {
                cursor.plan_gap(Gap::new(room_id.to_string(), from, to));
            }
            if !known {
                cursor.rooms.push(room_id.to_string());
            }
            events.push((room_id, timeline.events));
        }
        // A room the cursor knew that now shows only as an invitation or a knock (removed, then
        // invited again, inside this stretch): `/sync` says nothing of what it said before. That is
        // read down to where we were, as for a left room; a refusal gives the gap up by name.
        let away = response
            .rooms
            .invited
            .into_keys()
            .chain(response.rooms.knocked.into_keys());
        for room_id in away {
            if cursor.rooms.iter().any(|r| r == room_id.as_str()) {
                let gap = Gap::new(room_id.to_string(), next_batch.clone(), position.clone());
                cursor.plan_gap(gap);
            }
        }
        self.forget_rooms_not_joined(&mut cursor);
        // A cut with nothing open before says `more` once and reads the gap next time, so that one
        // fetch stays small. A cut with gaps open already does not: those carry the work, and a
        // server that cuts every answer must not keep `sync()` going.
        let defer = cut && !had_gaps;
        let mut unfinished = defer;
        if !defer {
            let (read, more) = self.read_gaps(&mut cursor).await?;
            events.extend(read);
            unfinished = more;
        }

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
        cursor.since = Some(next_batch);
        Ok(FetchBatch {
            messages,
            more: unfinished,
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
            let id = room.room_id().to_string();
            let known = cursor.rooms.contains(&id);
            let state = room.state();
            if !worth_replaying(state, known) {
                continue;
            }
            // A joined room the cursor does not know has not been read from its start: the fetch
            // that met it may have failed after the SDK took the answer. Its gap goes to the start.
            let to = cursor
                .since
                .clone()
                .filter(|_| !reads_to_start(state, known));
            cursor.plan_gap(Gap::new(id.clone(), position.to_owned(), to));
            if !known {
                cursor.rooms.push(id);
            }
        }
    }

    /// A room the account is no longer in (left, rejected, forgotten, banned) is not "read": when it
    /// comes back, what was said meanwhile has to be read as for a new room, not only down to the old
    /// position. Its open gaps stay: `read_gaps` reads what the server still gives.
    fn forget_rooms_not_joined(&self, cursor: &mut MatrixCursor) {
        cursor.rooms.retain(|id| {
            <&RoomId>::try_from(id.as_str())
                .ok()
                .and_then(|id| self.client.get_room(id))
                .is_some_and(|room| room.state() == RoomState::Joined)
        });
    }

    pub(super) fn room_context(&self, room_id: &RoomId) -> RoomContext {
        RoomContext {
            room_id: room_id.to_string(),
            name: self.client.get_room(room_id).and_then(|r| r.name()),
            own_user: self.account.user_id.clone(),
        }
    }
}
