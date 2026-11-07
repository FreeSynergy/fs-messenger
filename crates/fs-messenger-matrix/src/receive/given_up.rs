//! Gaps nothing can read any more, named in the cursor.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and `apply.rs`): `give_up` changes the cursor's snapshot, so it is not
//! a pure function.

use serde::{Deserialize, Serialize};

use super::cursor::{Gap, MatrixCursor};

fn is_earlier(why: &Why) -> bool {
    *why == Why::Earlier
}

#[allow(clippy::trivially_copy_pass_by_ref)] // the signature serde wants
fn is_once(n: &u32) -> bool {
    *n == 1
}

/// Given-up gaps the cursor names. The cursor is stored with every batch; the older ones are in the log.
pub(super) const MAX_GIVEN_UP: usize = 32;

impl MatrixCursor {
    /// This gap cannot be read any more, for this reason: one entry per room (merged, counted).
    #[allow(dead_code)] // a shell until the implementation
    pub(crate) fn give_up_why(&mut self, gap: Gap, why: Why) {
        let _ = (gap, why);
        unimplemented!("MatrixCursor::give_up_why")
    }

    /// This gap cannot be read any more: say so, by name, and drop it from the open ones. `why` is
    /// for the log only.
    pub(crate) fn give_up(&mut self, gap: Gap, why: &'static str) {
        tracing::warn!(room = %gap.room, from = %gap.from, why, "gap given up");
        self.given_up.push(GivenUp {
            room: gap.room,
            from: gap.from,
            to: gap.to,
            why: Why::Earlier,
            times: 1,
        });
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

/// One room with something unread that cannot be read: room and tokens, never content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GivenUp {
    pub(crate) room: String,
    /// The newest start of what was given up.
    pub(crate) from: String,
    /// The first (oldest) end.
    pub(crate) to: Option<String>,
    #[serde(default, skip_serializing_if = "is_earlier")]
    pub(crate) why: Why,
    /// How often this room was given up (merged into this one entry).
    #[serde(default = "once", skip_serializing_if = "is_once")]
    pub(crate) times: u32,
}

fn once() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(room: &str, from: &str, to: Option<&str>) -> Gap {
        Gap::new(room.into(), from.into(), to.map(Into::into))
    }

    #[test]
    fn a_room_given_up_again_is_one_entry_with_a_count() {
        let mut c = MatrixCursor::default();
        c.give_up_why(gap("!r:x", "t5", Some("t1")), Why::Forbidden);
        c.give_up_why(gap("!r:x", "t9", Some("t3")), Why::Forbidden);
        c.give_up_why(gap("!r:x", "t12", None), Why::Forbidden);
        assert_eq!(c.given_up.len(), 1);
        let one = &c.given_up[0];
        assert_eq!(
            (one.from.as_str(), one.to.as_deref(), one.times),
            ("t12", Some("t1"), 3)
        );
        assert_eq!(one.why, Why::Forbidden);
    }

    #[test]
    fn at_most_the_newest_rooms_are_named() {
        let mut c = MatrixCursor::default();
        for i in 0..MAX_GIVEN_UP + 8 {
            c.give_up_why(gap(&format!("!r{i}:x"), "t2", Some("t1")), Why::Overflow);
        }
        assert_eq!(c.given_up.len(), MAX_GIVEN_UP);
        assert_eq!(
            c.given_up.last().unwrap().room,
            format!("!r{}:x", MAX_GIVEN_UP + 7)
        );
        assert_eq!(c.given_up[0].room, "!r8:x", "the oldest went first");
    }
}
