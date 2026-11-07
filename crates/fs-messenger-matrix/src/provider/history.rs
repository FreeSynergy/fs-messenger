//! Reading what `/sync` did not bring: a limited timeline, a newly joined room, a replay.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points).
//! Reads the pages of `/messages`, resumable: where it stands is in the cursor. One fetch reads a
//! bounded number of events and says `more`, so a long room does not sit in memory whole.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::{OwnedRoomId, RoomId, UInt};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::MatrixCursor;

/// Events read through `/messages` in one fetch, and pages. Reaching the events limit means real
/// progress and says `more`; reaching the pages limit first means the pages were small, which is what
/// a server that never ends looks like: the fetch ends with the gap still in the cursor, and the next
/// call goes on. `PAGE` is also what is asked for per page, as `limit` is only a request.
const EVENTS_PER_FETCH: usize = 500;
const PAGES_PER_FETCH: usize = 20;
const PAGE: u32 = 100;

type Read = Vec<(OwnedRoomId, Vec<TimelineEvent>)>;

impl MatrixProvider {
    /// One fetch's worth of the planned gaps, each page oldest first; and whether another fetch
    /// should follow right away. A page with more events than asked for is cut, and the rest of it is
    /// read from the same token next time (`Gap::skip`).
    pub(super) async fn read_gaps(&self, cursor: &mut MatrixCursor) -> Result<(Read, bool)> {
        let mut events = Vec::new();
        let (mut budget, mut pages) = (EVENTS_PER_FETCH, 0);
        while let Some(gap) = cursor.gaps.last().cloned() {
            if budget == 0 {
                return Ok((events, true));
            }
            if pages == PAGES_PER_FETCH {
                break;
            }
            let room_id = <&RoomId>::try_from(gap.room.as_str())
                .map_err(|e| MatrixError::Cursor(e.to_string()))?;
            let Some(room) = self.client.get_room(room_id) else {
                cursor.gaps.pop();
                continue;
            };
            pages += 1;
            let mut options = MessagesOptions::backward().from(gap.from.as_str());
            options.to = gap.to;
            options.limit = UInt::from(PAGE);
            let page = self.account.bounded(room.messages(options)).await?;
            let seen = page.chunk.len();
            let skipped = gap.skip.min(seen);
            let mut chunk = page.chunk;
            chunk.drain(..skipped);
            let take = chunk.len().min(budget);
            let cut = chunk.len() > take;
            chunk.truncate(take);
            budget = budget.saturating_sub(take.max(1));
            if let Some(open) = cursor.gaps.last_mut() {
                if cut {
                    open.skip = skipped + take;
                } else if let Some(end) = page.end.filter(|_| seen > 0) {
                    open.from = end;
                    open.skip = 0;
                } else {
                    cursor.gaps.pop();
                }
            }
            chunk.reverse();
            events.push((room_id.to_owned(), chunk));
        }
        Ok((events, false))
    }
}
