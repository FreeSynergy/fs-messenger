//! One `/sync` answer as the table of `receive/membership.rs` reads it, whoever asked.
//!
//! Pattern: none, allowed by section 2 (plain data and the pure conversion `from_sdk` for one
//! concept: the answer). No foreign type is wrapped and no trait implemented, so it is no Adapter.
//! The SDK's answer and our own `/sync` (`raw_sync.rs`) both become this.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::sync::SyncResponse;

use crate::receive::Section;

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

/// The SDK's answer as ours. `own` is the account's Matrix id; `limit` the timeline limit.
pub(crate) fn from_sdk(response: SyncResponse, own: &str, limit: usize) -> Answer {
    let _ = (response, own, limit);
    unimplemented!("answer::from_sdk")
}
