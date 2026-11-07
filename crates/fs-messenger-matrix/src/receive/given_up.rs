//! Gaps nothing can read any more, named in the cursor.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and `apply.rs`): `give_up` changes the cursor's snapshot, so it is not
//! a pure function.

use serde::{Deserialize, Serialize};

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

#[allow(dead_code)] // a shell until the implementation
/// Why a gap was given up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Why {
    /// Given up by an earlier version, which did not say why.
    #[default]
    Earlier,
    /// The server answered `M_FORBIDDEN`.
    Forbidden,
    /// The SDK does not know the room.
    UnknownRoom,
    /// A check was refused: whether we ever joined is not known.
    CheckRefused,
    /// More checks than the cursor holds.
    Overflow,
}

#[allow(dead_code)] // a shell until the implementation
/// One room with something unread that cannot be read: room and tokens, never content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GivenUp {
    pub(crate) room: String,
    /// The newest start of what was given up.
    pub(crate) from: String,
    /// The first (oldest) end.
    pub(crate) to: Option<String>,
    #[serde(default)]
    pub(crate) why: Why,
    /// How often this room was given up (merged into this one entry).
    #[serde(default = "once")]
    pub(crate) times: u32,
}

#[allow(dead_code)] // a shell until the implementation
fn once() -> u32 {
    1
}
