//! Closing a gap: a `/sync` said its timeline is limited, so events are missing before it.
//!
//! Pattern: Iterator over the pages of `/messages`, oldest result first. Bounded, because a room can
//! be years long; what lies beyond the bound stays on the server (`DESIGN.md`).

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::{RoomId, UInt};

use super::MatrixProvider;
use crate::error::Result;

/// Events fetched per room to close one gap.
const MAX_EVENTS: usize = 5000;
const PAGE: u32 = 100;

impl MatrixProvider {
    /// What lies between `since` (or the start of the room) and `from`, oldest first.
    pub(super) async fn missing_events(
        &self,
        room_id: &RoomId,
        from: Option<String>,
        since: Option<&str>,
    ) -> Result<Vec<TimelineEvent>> {
        let (Some(mut from), Some(room)) = (from, self.client.get_room(room_id)) else {
            return Ok(vec![]);
        };
        let mut events = Vec::new();
        while events.len() < MAX_EVENTS {
            let mut options = MessagesOptions::backward().from(from.as_str());
            options.to = since.map(ToOwned::to_owned);
            options.limit = UInt::from(PAGE);
            let page = room.messages(options).await?;
            let empty = page.chunk.is_empty();
            events.extend(page.chunk);
            match page.end {
                Some(end) if !empty => from = end,
                _ => {
                    events.reverse();
                    return Ok(events);
                }
            }
        }
        tracing::warn!(room = %room_id, "gap larger than {MAX_EVENTS} events; older ones not fetched");
        events.reverse();
        Ok(events)
    }
}
