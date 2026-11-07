//! [`MatrixCursor`]: what the core keeps for this provider.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over this file, `gaps.rs`, `pending.rs`, `given_up.rs` and `apply.rs`): a snapshot of where this provider stands (the `/sync` token, the rooms read, the
//! open gaps, the events waiting for a key). The core keeps it as opaque text (`ProviderCursor`) and
//! hands it back to be read from. Being one value, the waiting list moves with the batch. The core stores each
//! message, then the cursor; a crash in between replays, and the core drops what it has.

use fs_messenger_core::ProviderCursor;
use serde::{Deserialize, Serialize};

use super::membership::Known;
use super::pending::Pending;
use crate::error::{MatrixError, where_json_failed};

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
    pub(crate) given_up: Vec<Gap>,
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
    #[allow(dead_code)] // a shell until the implementation
    pub(crate) fn known(&self, room: &str) -> Known {
        let _ = room;
        unimplemented!("MatrixCursor::known")
    }

    #[allow(dead_code)] // a shell until the implementation
    /// Set what this room is, within the bounds of the cursor.
    pub(crate) fn become_(&mut self, room: &str, known: Known) {
        let _ = (room, known);
        unimplemented!("MatrixCursor::become_")
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
}
