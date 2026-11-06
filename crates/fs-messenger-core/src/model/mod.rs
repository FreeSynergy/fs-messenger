//! Value types of the messenger; names mirror `messenger.capnp` (draft 0.2), no generated types.

mod account;
mod action;
mod attachment;
mod conversation;
mod event;
mod external_id;
mod kinds;
mod message;
mod page;
mod plan;
mod search;
mod tool_call;
mod usage;

pub use account::{Account, AccountSetup};
pub use action::MessageAction;
pub use attachment::{Attachment, Reaction};
pub use conversation::ConversationSummary;
pub use event::{Event, EventFilter, StateChange, TextChunk, TextPart, ToolCallChange};
pub use external_id::{ExternalId, MESSAGE_ID};
pub use kinds::{DeliveryState, Encryption, Kind, SenderKind, SignatureCheck};
pub use message::{Message, MessageSummary};
pub use page::{ConversationPage, Cursor, MessagePage, Page};
pub use plan::EncryptionPlan;
pub use search::SearchFilter;
pub use tool_call::{
    ToolCall, ToolCallChoice, ToolCallDecision, ToolCallKind, ToolCallOption, ToolCallOutcome,
    ToolCallRisk, ToolCallState,
};
pub use usage::Usage;
