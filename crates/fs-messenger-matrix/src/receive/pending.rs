//! The events that wait for a key, and the list that holds them.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`,
//! `pending.rs`, `given_up.rs` and `apply.rs`): the waiting list is one field of the cursor's
//! snapshot and moves with the batch; `wait_for` changes the cursor, so it is not a pure function.

use serde::{Deserialize, Serialize};

use super::alias::EventAlias;
use super::cursor::MatrixCursor;

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

impl MatrixCursor {
    pub(super) fn pending_bytes(&self) -> usize {
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
