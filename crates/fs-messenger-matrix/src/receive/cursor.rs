//! [`MatrixCursor`]: what the core keeps for this provider.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over this file, `gaps.rs`, `pending.rs`, `given_up.rs` and `apply.rs`): a snapshot of where this provider stands (the `/sync` token, the rooms read, the
//! open gaps, the events waiting for a key). The core keeps it as opaque text (`ProviderCursor`) and
//! hands it back to be read from. Being one value, the waiting list moves with the batch. The core stores each
//! message, then the cursor; a crash in between replays, and the core drops what it has.

use fs_messenger_core::ProviderCursor;
use serde::{Deserialize, Serialize};

use super::given_up::GivenUp;
use super::membership::Known;
use super::pending::Pending;
use crate::error::{MatrixError, where_json_failed};

/// Rooms we were in once and are not now, at most; the oldest go first and become unknown again,
/// which can only mean reading more later, never losing.
pub(crate) const MAX_AWAY: usize = 256;

/// Events still to be read through `/messages`, newest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Gap {
    pub(crate) room: String,
    /// Where the next page starts.
    pub(crate) from: String,
    /// Where it ends; `None` is the start of the room.
    pub(crate) to: Option<String>,
    /// Events of the page at `from` already delivered, newest first. A server may answer more events
    /// than were asked for; the rest of that page is then read from the same token in the next fetch.
    #[serde(default, skip_serializing_if = "skip_is_zero")]
    pub(crate) skip: usize,
    /// Set for a check: not events to read, but the question "is there an own join of ours in
    /// (`to`, `origin`]?". `from` is where the next page of that question starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) check: Option<CheckOf>,
}

/// What a check remembers while its pages are read over several fetches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CheckOf {
    /// The newest end of the stretch asked about: where reading starts once an own join is found.
    pub(crate) origin: String,
}

/// A room we have been in, and are not now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Away {
    pub(crate) room: String,
    /// The `since` of the fetch that saw the leave: the point before it. `None` is the room's start.
    pub(crate) at: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // the signature serde wants
fn skip_is_zero(n: &usize) -> bool {
    *n == 0
}

impl Gap {
    pub(crate) fn new(room: String, from: String, to: Option<String>) -> Self {
        Self {
            room,
            from,
            to,
            skip: 0,
            check: None,
        }
    }
}

/// Where this provider stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MatrixCursor {
    pub(crate) since: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) pending: Vec<Pending>,
    /// Rooms already read. A room not in here gets its visible history read: `/sync` starts it at
    /// the join, and what was said between the invitation and the join would be lost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) rooms: Vec<String>,
    /// Rooms we were in once and are not now, with the point to read back to when we return.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) away: Vec<Away>,
    /// What is still to be read, so that one fetch stays small and a long gap goes on in the next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) gaps: Vec<Gap>,
    /// Gaps nothing can read any more (the room is unknown to the SDK, or the server says `M_FORBIDDEN`), named so that it is known what
    /// was left unread: room and tokens, never content. The newest [`MAX_GIVEN_UP`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) given_up: Vec<GivenUp>,
}

impl MatrixCursor {
    pub(crate) fn read(cursor: Option<&ProviderCursor>) -> Result<Self, MatrixError> {
        match cursor {
            None => Ok(Self::default()),
            Some(c) => {
                serde_json::from_str(&c.0).map_err(|e| MatrixError::Cursor(where_json_failed(&e)))
            }
        }
    }

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

    pub(crate) fn write(&self) -> Result<ProviderCursor, MatrixError> {
        let text =
            serde_json::to_string(self).map_err(|e| MatrixError::Cursor(where_json_failed(&e)))?;
        Ok(ProviderCursor(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_survives_being_stored() {
        let mut c = MatrixCursor {
            since: Some("s1".into()),
            ..MatrixCursor::default()
        };
        c.rooms.push("!r:x".into());
        c.gaps
            .push(Gap::new("!r:x".into(), "t2".into(), Some("t1".into())));
        c.gaps[0].skip = 7;
        c.wait_for(Pending {
            room: "!r:x".into(),
            event: "$a".into(),
            raw: "{}".into(),
        });
        let written = c.write().unwrap();
        assert_eq!(MatrixCursor::read(Some(&written)).unwrap(), c);
        assert_eq!(MatrixCursor::read(None).unwrap(), MatrixCursor::default());
    }

    #[test]
    fn a_foreign_cursor_is_an_error_not_a_new_start() {
        assert!(MatrixCursor::read(Some(&ProviderCursor("nonsense".into()))).is_err());
    }

    #[test]
    fn a_cursor_of_the_earlier_format_still_reads_and_writes_the_same() {
        let old = r#"{"since":"s1","pending":[{"room":"!r:x","event":"$a","raw":"{}"}],"rooms":["!r:x"],"gaps":[{"room":"!r:x","from":"t2","to":"t1","skip":3}],"given_up":[{"room":"!g:x","from":"t9","to":"t1"}]}"#;
        let c = MatrixCursor::read(Some(&ProviderCursor(old.into()))).unwrap();
        assert!(c.away.is_empty());
        assert_eq!(c.gaps[0].check, None);
        assert_eq!(c.given_up[0].times, 1);
        let again = serde_json::to_string(&c).unwrap();
        let (a, b): (serde_json::Value, serde_json::Value) = (
            serde_json::from_str(old).unwrap(),
            serde_json::from_str(&again).unwrap(),
        );
        assert_eq!(a, b);
    }

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
