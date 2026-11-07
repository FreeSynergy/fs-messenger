//! Gaps nothing can read any more, named in the cursor.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and `apply.rs`): `give_up` changes the cursor's snapshot, so it is not
//! a pure function.

use super::cursor::{Gap, MatrixCursor};

/// Given-up gaps the cursor names. The cursor is stored with every batch; the older ones are in the log.
const MAX_GIVEN_UP: usize = 32;

impl MatrixCursor {
    /// This gap cannot be read any more: say so, by name, and drop it from the open ones. `why` is
    /// for the log only.
    pub(crate) fn give_up(&mut self, gap: Gap, why: &'static str) {
        tracing::warn!(room = %gap.room, from = %gap.from, why, "gap given up");
        self.given_up.push(gap);
        let over = self.given_up.len().saturating_sub(MAX_GIVEN_UP);
        self.given_up.drain(..over);
    }
}
