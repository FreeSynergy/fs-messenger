//! [`ConversationSummary`]: one line of the conversation list.
//!
//! Pattern: Value object, a read view; the stored form is in `repo/conversation_codec.rs`.

use fs_objects::ObjectId;
use serde::{Deserialize, Serialize};

use super::{Encryption, Kind, MessageSummary};

/// One conversation in a list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    /// Id; the `context` of its messages.
    pub id: ObjectId,
    /// Display name.
    pub name: String,
    /// Sort of conversation.
    pub kind: Kind,
    /// Encryption of the newest message; `unknown` while there is none.
    pub encryption: Encryption,
    /// Unread messages.
    pub unread_count: u32,
    /// The newest message, if any.
    pub last_message: Option<MessageSummary>,
    /// Whether it stays on top of the list.
    pub pinned: bool,
    /// Folder = group = distribution list (ADR 0037).
    pub folders: Vec<String>,
}
