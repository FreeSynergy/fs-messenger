//! The rooms we were in once and are not now.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `away.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and `apply.rs`): which list a room is in (member, away or neither) is
//! part of the cursor's snapshot, and `become_` changes it, so this is not a group of pure functions.

use serde::{Deserialize, Serialize};

use super::cursor::MatrixCursor;
use super::membership::Known;

/// Rooms we were in once and are not now, at most; the oldest go first and become unknown again,
/// which can only mean reading more later, never losing.
pub(crate) const MAX_AWAY: usize = 256;

/// A room we have been in, and are not now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Away {
    pub(crate) room: String,
    /// The `since` of the fetch that saw the leave: the point before it. `None` is the room's start.
    pub(crate) at: Option<String>,
}

impl MatrixCursor {
    /// What this room is to the cursor.
    pub(crate) fn known(&self, room: &str) -> Known {
        if self.rooms.iter().any(|r| r == room) {
            Known::Member
        } else if let Some(away) = self.away.iter().find(|a| a.room == room) {
            Known::Away {
                at: away.at.clone(),
            }
        } else {
            Known::Unknown
        }
    }

    /// Set what this room is. A room is in one list only. The away list is bounded: the oldest go
    /// first and are unknown again, which can only mean reading more later.
    pub(crate) fn become_(&mut self, room: &str, known: Known) {
        self.rooms.retain(|r| r != room);
        self.away.retain(|a| a.room != room);
        match known {
            Known::Unknown => {}
            Known::Member => self.rooms.push(room.to_owned()),
            Known::Away { at } => {
                self.away.push(Away {
                    room: room.to_owned(),
                    at,
                });
                let over = self.away.len().saturating_sub(MAX_AWAY);
                self.away.drain(..over);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_is_member_away_or_unknown_and_the_away_list_is_bounded() {
        let mut c = MatrixCursor::default();
        assert_eq!(c.known("!r:x"), Known::Unknown);
        c.become_("!r:x", Known::Member);
        assert_eq!(c.known("!r:x"), Known::Member);
        c.become_(
            "!r:x",
            Known::Away {
                at: Some("p".into()),
            },
        );
        assert_eq!(
            c.known("!r:x"),
            Known::Away {
                at: Some("p".into())
            }
        );
        assert!(c.rooms.is_empty(), "a room is in one list only");
        for i in 0..2 * MAX_AWAY {
            c.become_(&format!("!a{i}:x"), Known::Away { at: None });
        }
        assert_eq!(c.away.len(), MAX_AWAY);
        assert_eq!(c.known("!r:x"), Known::Unknown, "the oldest went first");
        assert_eq!(
            c.known(&format!("!a{}:x", 2 * MAX_AWAY - 1)),
            Known::Away { at: None }
        );
        c.become_("!a499:x", Known::Unknown);
        assert_eq!(c.known("!a499:x"), Known::Unknown);
    }
}
