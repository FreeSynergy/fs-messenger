//! Reading what `/sync` did not bring: a limited timeline, a newly joined room, a replay.
//!
//! Pattern: Iterator over the pages of `/messages`, resumable: where it stands is in the cursor. One
//! fetch reads a bounded number of events and says `more`, so a long room does not sit in memory whole.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::{OwnedRoomId, RoomId, UInt};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::MatrixCursor;

/// Events read through `/messages` in one fetch.
const EVENTS_PER_FETCH: u32 = 500;
const PAGE: u32 = 100;

impl MatrixProvider {
    /// One fetch's worth of the planned gaps, each page oldest first.
    pub(super) async fn read_gaps(
        &self,
        cursor: &mut MatrixCursor,
    ) -> Result<Vec<(OwnedRoomId, Vec<TimelineEvent>)>> {
        let mut events = Vec::new();
        let mut budget = EVENTS_PER_FETCH;
        while budget > 0 {
            let Some(gap) = cursor.gaps.last().cloned() else {
                break;
            };
            let room_id = <&RoomId>::try_from(gap.room.as_str())
                .map_err(|e| MatrixError::Cursor(e.to_string()))?;
            let Some(room) = self.client.get_room(room_id) else {
                cursor.gaps.pop();
                continue;
            };
            let mut options = MessagesOptions::backward().from(gap.from.as_str());
            options.to = gap.to;
            options.limit = UInt::from(budget.min(PAGE));
            let page = self.account.bounded(room.messages(options)).await?;
            let count = u32::try_from(page.chunk.len()).unwrap_or(u32::MAX);
            budget = budget.saturating_sub(count.max(1));
            match page.end {
                Some(end) if count > 0 => {
                    if let Some(open) = cursor.gaps.last_mut() {
                        open.from = end;
                    }
                }
                _ => {
                    cursor.gaps.pop();
                }
            }
            let mut chunk = page.chunk;
            chunk.reverse();
            events.push((room_id.to_owned(), chunk));
        }
        Ok(events)
    }
}
