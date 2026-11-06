//! What a provider delivers: [`RawMessage`], [`FetchBatch`].
//!
//! Pattern: Value objects at the boundary. Unset encryption reads as `unknown`; the provider must
//! say what it knows, and the core never assumes more.

use chrono::{DateTime, Utc};
use fs_store::Blob;
use serde::{Deserialize, Serialize};

use crate::model::{Encryption, ExternalId, Kind, ToolCall};

/// Where a provider stands in its own stream. Opaque to the core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderCursor(pub String);

/// One answer to `Provider::fetch`.
#[derive(Clone, Debug, Default)]
pub struct FetchBatch {
    /// The messages.
    pub messages: Vec<RawMessage>,
    /// Where to go on from; `None` keeps the old position.
    pub next: Option<ProviderCursor>,
    /// Whether another fetch would deliver more right now.
    pub more: bool,
}

/// A file as delivered.
#[derive(Clone, Debug)]
pub struct RawAttachment {
    /// File name.
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// The bytes.
    pub data: Vec<u8>,
}

/// A tool call as an agent reports it.
#[derive(Clone, Debug, Default)]
pub struct RawToolCall {
    /// The agent's own reference, not empty; never used as `ToolCall.id`.
    pub agent_ref: String,
    /// The call. `id`, `risk`, `options`, `arguments_digest`, `asked_of` and `revision` are the
    /// engine's and not taken from here; `detail` moves into `summary`.
    pub call: ToolCall,
}

/// One message as a provider delivers it.
#[derive(Clone, Debug)]
pub struct RawMessage {
    /// What the provider calls it; at least one. A provider without ids must create its own.
    pub aliases: Vec<ExternalId>,
    /// Room, mail thread, comment thread.
    pub conversation: ExternalId,
    /// Name of the conversation, if a new one has to be created.
    pub conversation_name: Option<String>,
    /// Sort of message.
    pub kind: Kind,
    /// How it arrived. Unset is `unknown`.
    pub encryption: Encryption,
    /// Sender's address or actor IRI.
    pub sender: String,
    /// Primary recipients.
    pub to: Vec<String>,
    /// Secondary recipients.
    pub cc: Vec<String>,
    /// Subject; becomes the envelope's `name`.
    pub subject: Option<String>,
    /// When it was written; `None` is now.
    pub published: Option<DateTime<Utc>>,
    /// The parent, in the provider's terms.
    pub in_reply_to: Option<ExternalId>,
    /// Normalised text.
    pub body_text: String,
    /// Media type of the text; empty is `text/plain`.
    pub body_media_type: String,
    /// Files.
    pub attachments: Vec<RawAttachment>,
    /// The message as received, byte for byte (ADR 0030).
    pub original: Option<Blob>,
    /// What an agent is doing.
    pub tool_calls: Vec<RawToolCall>,
    /// Uids of tasks it refers to.
    pub task_refs: Vec<String>,
    /// Reasoning of an agent.
    pub reasoning: String,
    /// The job it is anchored to.
    pub job_ref: String,
    /// Whether I wrote it, for example on another device.
    pub outgoing: bool,
}

impl RawMessage {
    /// A message with what every message needs; the rest stays at its fail-safe default.
    pub fn new(
        alias: ExternalId,
        conversation: ExternalId,
        sender: impl Into<String>,
        body_text: impl Into<String>,
    ) -> Self {
        Self {
            aliases: vec![alias],
            conversation,
            conversation_name: None,
            kind: Kind::default(),
            encryption: Encryption::default(),
            sender: sender.into(),
            to: vec![],
            cc: vec![],
            subject: None,
            published: None,
            in_reply_to: None,
            body_text: body_text.into(),
            body_media_type: String::new(),
            attachments: vec![],
            original: None,
            tool_calls: vec![],
            task_refs: vec![],
            reasoning: String::new(),
            job_ref: String::new(),
            outgoing: false,
        }
    }
}
