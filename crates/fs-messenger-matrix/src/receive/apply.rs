//! Applying one answer to the cursor.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and this file): `apply` walks the rooms of an answer, asks
//! `membership::step` and writes what it decides into the cursor's snapshot.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::OwnedRoomId;

use super::cursor::MatrixCursor;
use super::membership::{Known, Seen, step};
use crate::provider::Answer;

impl MatrixCursor {
    /// Apply an answer asked from `since`: plan the gaps and checks, set the states, and hand out the
    /// timelines that may be delivered (and only those: a room never accepted delivers nothing).
    pub(crate) fn apply(
        &mut self,
        answer: &mut Answer,
        since: Option<&str>,
    ) -> Vec<(OwnedRoomId, Vec<TimelineEvent>)> {
        let next = answer.next_batch.clone();
        let mut delivered = Vec::new();
        for room in &mut answer.rooms {
            let id = room.id.to_string();
            let known = self.known(&id);
            let seen = Seen {
                section: room.section,
                own_join: room.own_join,
                limited: room.limited,
                cut: room.cut,
                prev_batch: room.prev_batch.as_deref(),
            };
            let moved = step(&known, &seen, &id, since, &next);
            self.plan_gap_if(moved.read);
            self.plan_gap_if(moved.check);
            // What is read from the start, or down to the point of the leave, covers any open check.
            let accepted = known == Known::Unknown && moved.becomes != Known::Unknown;
            if accepted || moved.becomes == Known::Member {
                self.gaps.retain(|g| !(g.room == id && g.check.is_some()));
            }
            if moved.becomes != known {
                self.become_(&id, moved.becomes);
            }
            if moved.deliver {
                delivered.push((room.id.clone(), std::mem::take(&mut room.events)));
            }
        }
        delivered
    }
}
