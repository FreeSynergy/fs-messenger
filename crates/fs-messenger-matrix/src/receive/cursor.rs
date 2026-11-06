//! [`MatrixCursor`]: what the core keeps for this provider.
//!
//! Pattern: Value object. The `/sync` token, and the events still waiting for a key. Being one value,
//! the waiting list moves with the batch: the core stores both, or neither, and a crash replays.

use fs_messenger_core::ProviderCursor;
use serde::{Deserialize, Serialize};

use crate::error::MatrixError;

/// Events waiting for a key, at most. An older one stays a placeholder for good; `DESIGN.md`.
const MAX_PENDING: usize = 256;

/// An event that could not be decrypted, as it arrived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub(crate) room: String,
    pub(crate) event: String,
    pub(crate) raw: String,
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
}

impl MatrixCursor {
    pub(crate) fn read(cursor: Option<&ProviderCursor>) -> Result<Self, MatrixError> {
        match cursor {
            None => Ok(Self::default()),
            Some(c) => serde_json::from_str(&c.0).map_err(|e| MatrixError::Cursor(e.to_string())),
        }
    }

    pub(crate) fn write(&self) -> Result<ProviderCursor, MatrixError> {
        let text = serde_json::to_string(self).map_err(|e| MatrixError::Cursor(e.to_string()))?;
        Ok(ProviderCursor(text))
    }

    /// Wait for this event's key too. A replay does not add it twice.
    pub(crate) fn wait_for(&mut self, pending: Pending) {
        if self.pending.iter().any(|p| p.event == pending.event) {
            return;
        }
        self.pending.push(pending);
        if self.pending.len() > MAX_PENDING {
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
