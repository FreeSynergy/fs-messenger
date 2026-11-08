//! Our own `/sync`, asked from a token of our choosing.
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder):
//! `Client::send` of a `/sync` request, the SDK's `Room::decrypt_event`, and the answer turned into
//! our `Answer`. The SDK does not process this answer: it is only read, so asking from an older
//! token than the SDK's own is no trouble for it.

use std::future::IntoFuture;
use std::time::Duration;

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::ruma::api::client::sync::sync_events::v3::{self, Filter, State};
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::serde::Raw;

use super::MatrixProvider;
use super::answer::{Answer, RoomAnswer};
use crate::error::Result;
use crate::receive::{Section, any_own_join};

type Timeline = v3::Timeline;

/// Whether an own join is in this room's state or timeline.
fn own_join_in(own: &str, room: &str, state: &State, timeline: &Timeline) -> bool {
    let state = match state {
        State::Before(s) | State::After(s) => s.events.iter().map(|e| e.json().get()).collect(),
        _ => Vec::new(),
    };
    let events = timeline.events.iter().map(|e| e.json().get());
    any_own_join(own, room, state.into_iter().chain(events))
}

impl MatrixProvider {
    /// `GET /sync?since=...` with the filter of `sync_once`, under `bounded`. Plain events are taken
    /// as they are, encrypted ones go through `Room::decrypt_event` (without a key: a placeholder
    /// that waits). `limit` is the timeline limit: a room with more events is cut to the newest.
    pub(super) async fn sync_from(&self, since: Option<&str>, limit: usize) -> Result<Answer> {
        let mut request = v3::Request::new();
        request.filter = Some(Filter::FilterDefinition(self.sync_filter()));
        request.since = since.map(ToOwned::to_owned);
        request.timeout = Some(Duration::ZERO);
        request.set_presence = PresenceState::Offline;
        let sent = self.client.send(request).into_future();
        let response = self.account.bounded(sent).await?;
        let own = self.account.user_id.as_str();
        let mut rooms = Vec::new();
        let sections = [
            (
                Section::Join,
                response
                    .rooms
                    .join
                    .into_iter()
                    .map(|(i, r)| (i, r.state, r.timeline))
                    .collect::<Vec<_>>(),
            ),
            (
                Section::Leave,
                response
                    .rooms
                    .leave
                    .into_iter()
                    .map(|(i, r)| (i, r.state, r.timeline))
                    .collect(),
            ),
        ];
        for (section, listed) in sections {
            for (id, state, t) in listed {
                let own_join = own_join_in(own, id.as_str(), &state, &t);
                let events = self.timeline_events(&id, t.events).await?;
                let room = RoomAnswer::new(
                    id,
                    section,
                    events,
                    t.limited,
                    t.prev_batch,
                    own_join,
                    limit,
                );
                rooms.push(room);
            }
        }
        let invited = response.rooms.invite.into_keys();
        rooms.extend(invited.map(|id| RoomAnswer::only(id, Section::Invite)));
        let knocked = response.rooms.knock.into_keys();
        rooms.extend(knocked.map(|id| RoomAnswer::only(id, Section::Knock)));
        Ok(Answer {
            next_batch: response.next_batch,
            rooms,
        })
    }

    /// The events of one room's timeline as the SDK would hand them on: an encrypted one opened if
    /// the SDK knows the room, else as it is (`convert` waits for its key).
    async fn timeline_events(
        &self,
        room_id: &OwnedRoomId,
        raws: Vec<Raw<AnySyncTimelineEvent>>,
    ) -> Result<Vec<TimelineEvent>> {
        let room = self.client.get_room(room_id);
        let mut events = Vec::with_capacity(raws.len());
        for raw in raws {
            let kind = raw.get_field::<String>("type").ok().flatten();
            match &room {
                Some(room) if kind.as_deref() == Some("m.room.encrypted") => {
                    let sealed: Raw<OriginalSyncRoomEncryptedEvent> =
                        Raw::from_json(raw.into_json());
                    events.push(room.decrypt_event(&sealed, None).await?);
                }
                _ => events.push(TimelineEvent::from_plaintext(raw)),
            }
        }
        Ok(events)
    }
}
