//! [`ExternalId`]: a message's id in somebody else's system.
//!
//! Pattern: Value object: the row of the alias table in `data/messages/technical.md`.

use serde::{Deserialize, Serialize};

/// What another system calls a message or a conversation: `Message-ID`, Matrix `event_id`, ...
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalId {
    /// Whose id this is, for example `message-id`, `matrix`, `mimi`. Must not contain `|`.
    pub scheme: String,
    /// The id in that scheme.
    pub id: String,
}

impl ExternalId {
    /// An id in `scheme`.
    pub fn new(scheme: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            id: id.into(),
        }
    }
}
