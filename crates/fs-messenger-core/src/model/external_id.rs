//! [`ExternalId`]: a message's id in somebody else's system.
//!
//! Pattern: Value object: the row of the alias table in `data/messages/technical.md`.

use serde::{Deserialize, Serialize};

/// The scheme of the mail header `Message-ID`, which whoever sends the mail writes.
pub const MESSAGE_ID: &str = "message-id";

/// What another system calls a message or a conversation: `Message-ID`, Matrix `event_id`, ...
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalId {
    /// Whose id this is, for example `message-id`, `matrix`, `mimi`. Must not contain `|`.
    pub scheme: String,
    /// The id in that scheme.
    pub id: String,
}

impl ExternalId {
    /// Whether the sender chose this id, so that it can only link and never dedupes.
    pub fn is_sender_chosen(&self) -> bool {
        self.scheme.eq_ignore_ascii_case(MESSAGE_ID)
    }

    /// An id in `scheme`.
    pub fn new(scheme: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            id: id.into(),
        }
    }
}
