//! Applying one answer to the cursor.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and this file): `apply` walks the rooms of an answer, asks
//! `membership::step` and writes what it decides into the cursor's snapshot.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::OwnedRoomId;

use super::cursor::MatrixCursor;
use crate::provider::Answer;

impl MatrixCursor {
    /// Apply an answer asked from `since`: plan the gaps and checks, set the states, and hand out the
    /// timelines that may be delivered.
    pub(crate) fn apply(
        &mut self,
        answer: &mut Answer,
        since: Option<&str>,
    ) -> Vec<(OwnedRoomId, Vec<TimelineEvent>)> {
        let _ = (answer, since);
        unimplemented!("MatrixCursor::apply")
    }
}
