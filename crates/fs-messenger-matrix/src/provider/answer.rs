//! One `/sync` answer as the table of `receive/membership.rs` reads it, whoever asked.
//!
//! Pattern: none, allowed by section 2 (plain data and the pure conversion `from_sdk` for one
//! concept: the answer). No foreign type is wrapped and no trait implemented, so it is no Adapter.
//! The SDK's answer and our own `/sync` (`raw_sync.rs`) both become this.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::sync::SyncResponse;
use matrix_sdk_base::sync::State;

use crate::receive::{Section, any_own_join};

/// What the answer says of one room.
#[derive(Clone, Debug)]
pub(crate) struct RoomAnswer {
    pub(crate) id: OwnedRoomId,
    pub(crate) section: Section,
    /// The timeline, at most the limit asked for (the newest).
    pub(crate) events: Vec<TimelineEvent>,
    pub(crate) limited: bool,
    /// The timeline had more events than the limit and was cut.
    pub(crate) cut: bool,
    pub(crate) prev_batch: Option<String>,
    /// An own join is in the state or the timeline (`receive/evidence.rs`).
    pub(crate) own_join: bool,
}

impl RoomAnswer {
    /// A room's part of an answer. A timeline over `limit` is cut to its newest events: `limit` is
    /// only a request to the server.
    pub(crate) fn new(
        id: OwnedRoomId,
        section: Section,
        mut events: Vec<TimelineEvent>,
        limited: bool,
        prev_batch: Option<String>,
        own_join: bool,
        limit: usize,
    ) -> Self {
        let cut = events.len() > limit;
        if cut {
            let over = events.len() - limit;
            events.drain(..over);
        }
        Self {
            id,
            section,
            events,
            limited,
            cut,
            prev_batch,
            own_join,
        }
    }

    /// A room that is only an invitation or a knock: nothing but the fact.
    pub(crate) fn only(id: OwnedRoomId, section: Section) -> Self {
        Self::new(id, section, Vec::new(), false, None, false, 0)
    }
}

/// A whole answer.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub(crate) next_batch: String,
    pub(crate) rooms: Vec<RoomAnswer>,
}

impl Answer {
    /// Whether any room's timeline was cut.
    pub(crate) fn cut(&self) -> bool {
        self.rooms.iter().any(|r| r.cut)
    }
}

fn state_jsons(state: &State) -> Vec<&str> {
    let (State::Before(events) | State::After(events)) = state;
    events.iter().map(|e| e.json().get()).collect()
}

fn own_join_in(own: &str, state: &State, timeline: &[TimelineEvent]) -> bool {
    let events = timeline.iter().map(|e| e.raw().json().get());
    any_own_join(own, state_jsons(state).into_iter().chain(events))
}

/// The SDK's answer as ours. `own` is the account's Matrix id; `limit` the timeline limit.
pub(crate) fn from_sdk(response: SyncResponse, own: &str, limit: usize) -> Answer {
    let mut rooms = Vec::new();
    for (id, update) in response.rooms.joined {
        let own_join = own_join_in(own, &update.state, &update.timeline.events);
        let t = update.timeline;
        let room = RoomAnswer::new(
            id,
            Section::Join,
            t.events,
            t.limited,
            t.prev_batch,
            own_join,
            limit,
        );
        rooms.push(room);
    }
    for (id, update) in response.rooms.left {
        let own_join = own_join_in(own, &update.state, &update.timeline.events);
        let t = update.timeline;
        let room = RoomAnswer::new(
            id,
            Section::Leave,
            t.events,
            t.limited,
            t.prev_batch,
            own_join,
            limit,
        );
        rooms.push(room);
    }
    rooms.extend(
        response
            .rooms
            .invited
            .into_keys()
            .map(|id| RoomAnswer::only(id, Section::Invite)),
    );
    rooms.extend(
        response
            .rooms
            .knocked
            .into_keys()
            .map(|id| RoomAnswer::only(id, Section::Knock)),
    );
    Answer {
        next_batch: response.next_batch,
        rooms,
    }
}
