//! [`MatrixCursor`]: what the core keeps for this provider.
//!
//! Pattern: Memento: a snapshot of where this provider stands (the `/sync` token, the rooms read, the
//! open gaps, the events waiting for a key). The core keeps it as opaque text (`ProviderCursor`) and
//! hands it back to be read from. Being one value, the waiting list moves with the batch: the core stores both, or neither, and a crash replays.

use fs_messenger_core::ProviderCursor;
use serde::{Deserialize, Serialize};

use super::alias::EventAlias;
use crate::error::{MatrixError, where_json_failed};

/// Events waiting for a key, at most, and their bytes together: the cursor is stored with every batch.
/// An older one stays a placeholder for good; `DESIGN.md`.
const MAX_PENDING: usize = 256;
const MAX_PENDING_BYTES: usize = 1024 * 1024;
/// What the Matrix specification allows for one event. Anything larger does not wait.
const MAX_EVENT_BYTES: usize = 65_536;

/// An event that could not be decrypted, as it arrived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub(crate) room: String,
    pub(crate) event: String,
    pub(crate) raw: String,
}

impl Pending {
    /// The alias the core knows this event under.
    pub(crate) fn alias(&self) -> EventAlias {
        EventAlias::new(&self.room, &self.event)
    }
}

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
    /// What is still to be read, so that one fetch stays small and a long gap goes on in the next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) gaps: Vec<Gap>,
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

    pub(crate) fn write(&self) -> Result<ProviderCursor, MatrixError> {
        let text =
            serde_json::to_string(self).map_err(|e| MatrixError::Cursor(where_json_failed(&e)))?;
        Ok(ProviderCursor(text))
    }

    fn pending_bytes(&self) -> usize {
        self.pending.iter().map(|p| p.raw.len()).sum()
    }

    /// Whether this event is a placeholder in the core that still waits for its key.
    pub(crate) fn is_waiting(&self, alias: &EventAlias) -> bool {
        self.pending.iter().any(|p| p.alias() == *alias)
    }

    /// Wait for this event's key too. A replay does not add it twice.
    pub(crate) fn wait_for(&mut self, pending: Pending) {
        if pending.raw.len() > MAX_EVENT_BYTES || self.is_waiting(&pending.alias()) {
            return;
        }
        self.pending.push(pending);
        while self.pending.len() > MAX_PENDING || self.pending_bytes() > MAX_PENDING_BYTES {
            self.pending.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(event: &str) -> Pending {
        Pending {
            room: "!r:x".into(),
            event: event.into(),
            raw: "{}".into(),
        }
    }

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
        c.wait_for(pending("$a"));
        let written = c.write().unwrap();
        assert_eq!(MatrixCursor::read(Some(&written)).unwrap(), c);
        assert_eq!(MatrixCursor::read(None).unwrap(), MatrixCursor::default());
    }

    #[test]
    fn a_foreign_cursor_is_an_error_not_a_new_start() {
        assert!(MatrixCursor::read(Some(&ProviderCursor("nonsense".into()))).is_err());
    }

    #[test]
    fn the_list_is_bounded_in_bytes_and_refuses_what_no_event_can_be() {
        let mut c = MatrixCursor::default();
        let big = |e: &str, n| Pending {
            room: "!r:x".into(),
            event: e.into(),
            raw: "x".repeat(n),
        };
        c.wait_for(big("$huge", MAX_EVENT_BYTES + 1));
        assert_eq!(c.pending, []);
        for i in 0..40 {
            c.wait_for(big(&format!("$e{i}"), MAX_EVENT_BYTES));
        }
        assert!(c.pending_bytes() <= MAX_PENDING_BYTES);
        assert_eq!(c.pending.last().unwrap().event, "$e39", "the newest stays");
    }

    #[test]
    fn the_same_event_id_in_another_room_is_another_event() {
        let mut c = MatrixCursor::default();
        let mut other = pending("$a");
        other.room = "!other:x".into();
        c.wait_for(pending("$a"));
        c.wait_for(other);
        assert_eq!(c.pending.len(), 2, "room and event id make the alias");
    }

    #[test]
    fn the_list_is_bounded_and_has_no_doubles() {
        let mut c = MatrixCursor::default();
        c.wait_for(pending("$a"));
        c.wait_for(pending("$a"));
        assert_eq!(c.pending.len(), 1);
        for i in 0..2 * MAX_PENDING {
            c.wait_for(pending(&format!("$e{i}")));
        }
        assert_eq!(c.pending.len(), MAX_PENDING);
    }
}
