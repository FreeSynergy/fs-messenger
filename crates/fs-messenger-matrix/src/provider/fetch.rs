//! One sync: what happened since the cursor, as `RawMessage`s.
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder): `fetch` of the `Provider` trait over the SDK's `/sync`. One
//! fetch, in order: ask (the SDK's `/sync`, or our own from the cursor if the SDK stands ahead of
//! it), apply the answer to the cursor (one decision table, `receive/membership.rs`), read gaps and
//! checks, convert, retry what waits for a key, say where we are. A failure anywhere fails the fetch,
//! so the cursor stays and the next one asks again from it; the core dedupes.

use std::time::Duration;

use fs_messenger_core::{FetchBatch, ProviderCursor};
use matrix_sdk::config::{SyncSettings, SyncToken};
use matrix_sdk::ruma::api::client::filter::FilterDefinition;
use matrix_sdk::ruma::api::client::sync::sync_events::v3::Filter;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{RoomId, UInt};
use matrix_sdk::store::{StateStoreDataKey, StateStoreDataValue};

use super::MatrixProvider;
use super::answer::{Answer, RoomAnswer, from_sdk};
use crate::error::{MatrixError, Result};
use crate::receive::{EventAlias, MatrixCursor, RoomContext};

impl MatrixProvider {
    /// The filter of every `/sync` of ours: the timeline limit per room.
    pub(super) fn sync_filter(&self) -> FilterDefinition {
        let mut filter = FilterDefinition::default();
        filter.room.timeline.limit = Some(UInt::from(self.account.timeline_limit));
        filter
    }

    fn timeline_limit(&self) -> usize {
        usize::try_from(self.account.timeline_limit).unwrap_or(usize::MAX)
    }

    fn sync_settings(&self, since: Option<String>) -> SyncSettings {
        let token = since.map_or(SyncToken::NoToken, SyncToken::Specific);
        SyncSettings::new()
            .token(token)
            .timeout(Duration::ZERO)
            .set_presence(PresenceState::Offline)
            .filter(Filter::FilterDefinition(self.sync_filter()))
    }

    /// The SDK's own position, not the core's: the SDK ignores a response it has seen, so asking it
    /// from an older cursor would answer nothing.
    async fn sdk_position(&self) -> Result<Option<String>> {
        let token = self
            .client
            .state_store()
            .get_kv_data(StateStoreDataKey::SyncToken)
            .await
            .map_err(|e| MatrixError::Sdk(e.to_string()))?;
        Ok(token.and_then(StateStoreDataValue::into_sync_token))
    }

    /// What happened since `asked`. If the SDK stands ahead of the cursor (`replay`: a fetch failed
    /// after the SDK took its answer, or the core did not keep the cursor), the stretch is asked
    /// for again, by our own `/sync`, not guessed from what the SDK holds now.
    async fn ask(&self, replay: bool, asked: Option<&str>) -> Result<Answer> {
        let limit = self.timeline_limit();
        if replay {
            return self.sync_from(asked, limit).await;
        }
        let settings = self.sync_settings(asked.map(ToOwned::to_owned));
        let response = self
            .account
            .bounded(self.client.sync_once(settings))
            .await?;
        Ok(from_sdk(response, &self.account.user_id, limit))
    }

    /// Whether the SDK holds this room.
    fn holds(&self, id: &str) -> bool {
        let id = <&RoomId>::try_from(id).ok();
        id.is_some_and(|id| self.client.get_room(id).is_some())
    }

    /// A room the SDK does not hold any more (forgotten, or its store was lost) is not remembered
    /// as ours: it is unknown again, which can only mean reading more when it returns.
    fn forget_unheld(&self, cursor: &mut MatrixCursor) {
        cursor.rooms.retain(|r| self.holds(r));
        cursor.away.retain(|a| self.holds(&a.room));
    }

    /// In a replay our own answer reaches past the SDK's position, so a room that the cursor never
    /// accepted and the SDK does not hold yet is one the SDK has not seen: left out here, it is
    /// neither planned nor given up, and the ordinary fetch from the SDK's position brings it.
    fn leave_to_the_sdk(&self, cursor: &MatrixCursor, answer: &mut Answer) {
        let id = |r: &RoomAnswer| r.id.to_string();
        answer
            .rooms
            .retain(|r| cursor.accepted(&id(r)) || self.holds(&id(r)));
    }

    pub(super) async fn fetch_batch(&self, since: Option<&ProviderCursor>) -> Result<FetchBatch> {
        let mut cursor = MatrixCursor::read(since)?;
        // Held for the whole fetch: see `echo.rs`.
        let sent = self.sent.lock().await;
        let position = self.sdk_position().await?;
        let replay = position.is_some() && position != cursor.since;
        let asked = if replay {
            cursor.since.clone()
        } else {
            position.clone()
        };
        let mut answer = self.ask(replay, asked.as_deref()).await?;
        if replay {
            self.leave_to_the_sdk(&cursor, &mut answer);
        }
        let next_batch = answer.next_batch.clone();
        let (had_gaps, cut) = (!cursor.gaps.is_empty(), answer.cut());
        let mut events = cursor.apply(&mut answer, asked.as_deref());
        self.forget_unheld(&mut cursor);
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
        // After a replay the cursor stands where the SDK does; the stretch from there to our own
        // answer's end is asked for again by the ordinary fetch that `more` brings at once.
        cursor.since = if replay { position } else { Some(next_batch) };
        Ok(FetchBatch {
            messages,
            more: unfinished || replay,
            next: Some(cursor.write()?),
        })
    }

    pub(super) fn room_context(&self, room_id: &RoomId) -> RoomContext {
        RoomContext {
            room_id: room_id.to_string(),
            name: self.client.get_room(room_id).and_then(|r| r.name()),
            own_user: self.account.user_id.clone(),
        }
    }
}
