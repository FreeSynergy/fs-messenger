//! [`Event`]s of the change feed and their [`EventFilter`].
//!
//! Pattern: Observer (data side): what a `Listener` receives.

use fs_store::ObjectId;
use serde::{Deserialize, Serialize};

use super::{DeliveryState, Kind, MessageSummary, ToolCall};

/// Which events a listener wants. Empty lists mean all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventFilter {
    /// Only these conversations.
    pub conversations: Vec<ObjectId>,
    /// Only these kinds.
    pub kinds: Vec<Kind>,
}

/// The delivery state of a message changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateChange {
    /// The message.
    pub message: ObjectId,
    /// Its new state.
    pub state: DeliveryState,
}

/// One tool call changed; it replaces the entry with the same id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallChange {
    /// The message.
    pub message: ObjectId,
    /// The call as it is now.
    pub call: ToolCall,
    /// The conversation of the message.
    pub conversation: ObjectId,
}

/// Which text a chunk belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextPart {
    /// The answer.
    Body,
    /// The reasoning, kept apart from the answer.
    Reasoning,
}

/// A piece of text while an agent writes it. In the contract; never emitted in this version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextChunk {
    /// The message.
    pub message: ObjectId,
    /// Which text.
    pub part: TextPart,
    /// UTF-8 bytes within this part, on whole code points.
    pub offset: u64,
    /// The piece.
    pub text: String,
    /// The conversation of the message.
    pub conversation: ObjectId,
}

/// One change a listener hears about.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A new message.
    Created(MessageSummary),
    /// A message changed in another way than the variants below.
    Updated(MessageSummary),
    /// A message was deleted.
    Deleted(ObjectId),
    /// The delivery state changed.
    State(StateChange),
    /// One tool call changed.
    ToolCall(ToolCallChange),
    /// A piece of text. Never emitted in this version: nothing streams yet.
    Text(TextChunk),
}
