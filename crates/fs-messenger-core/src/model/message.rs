//! [`Message`] and [`MessageSummary`], named as in the contract.
//!
//! Pattern: Value objects, a read view of envelope + content + immutable original (ADR 0030). The
//! stored form is in `repo/message_codec.rs`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fs_store::{Blob, Envelope, ObjectId};
use serde::{Deserialize, Serialize};

use super::{
    Attachment, DeliveryState, Encryption, ExternalId, Kind, Reaction, SenderKind, ToolCall,
};

/// What a message list shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    /// AS2 envelope: `id`, `context` = conversation, `inReplyTo`, addressing, times.
    pub envelope: Envelope,
    /// Sort of message.
    pub kind: Kind,
    /// How it is protected; `none` and `unknown` are never shown as encrypted (ADR 0018).
    pub encryption: Encryption,
    /// Delivery state.
    pub state: DeliveryState,
    /// Whether I still have to read it.
    pub unread: bool,
    /// Whether I flagged it.
    pub flagged: bool,
    /// Plain-text snippet.
    pub preview: String,
    /// Who wrote it (ADR 0054).
    pub sender_kind: SenderKind,
    /// The job this message is anchored to; empty if none.
    pub job_ref: String,
}

/// A whole message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The list view.
    pub summary: MessageSummary,
    /// Normalised text.
    pub body_text: String,
    /// Sanitised HTML. Always empty in this version: there is no sanitiser yet, and the raw form
    /// stays in `original`.
    pub body_html: String,
    /// Files.
    pub attachments: Vec<Attachment>,
    /// Reactions.
    pub reactions: Vec<Reaction>,
    /// As received, byte for byte (ADR 0030).
    pub original: Option<Blob>,
    /// Media type of `original`; empty without one.
    pub original_media_type: String,
    /// What an agent is doing (ADR 0054). Stored data in this version.
    pub tool_calls: Vec<ToolCall>,
    /// Uids of tasks this message refers to.
    pub task_refs: Vec<String>,
    /// Reasoning of an agent, kept apart from the answer.
    pub reasoning: String,
    /// `text/plain` or `text/markdown`.
    pub body_media_type: String,
    /// What other systems call this message. Not in the contract; the dedupe key.
    pub aliases: Vec<ExternalId>,
    /// The account it belongs to. Not in the contract.
    pub account: Option<ObjectId>,
    /// Whether I sent it. Not in the contract.
    pub outgoing: bool,
    /// The parent, while it has not arrived yet. Not in the contract.
    pub pending_reply: Option<ExternalId>,
    /// Local arrival sequence, never from a sender. "Read up to" runs over it. Not in the contract.
    pub arrival: u64,
    /// When to sort it instead of `published`, if the sender dated it in the future. Not in the
    /// contract.
    pub sort_at: Option<DateTime<Utc>>,
    /// The agent's own reference of a call -> `ToolCall.id`. Not in the contract.
    pub tool_call_refs: BTreeMap<String, String>,
}

impl Message {
    /// The id; the first version's id stays on edit.
    pub fn id(&self) -> ObjectId {
        self.summary.envelope.id
    }

    /// The time it sorts by: the sender's date, unless that is later than its arrival.
    pub fn sort_time(&self) -> DateTime<Utc> {
        self.sort_at.unwrap_or(self.summary.envelope.published)
    }
}
