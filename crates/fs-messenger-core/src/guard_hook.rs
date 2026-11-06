//! What the core asks of a guard: the decisions and stops it routes, and who a sender is.
//!
//! Pattern: Strategy: the core depends on these traits and never on the crate `fs-messenger-guard`,
//! which depends on the core. A `Provider` never sees them. Without an installed guard the core answers
//! `Unsupported` for `decide` and `stop`, which is the safe default.

use async_trait::async_trait;
use fs_store::ObjectId;

use crate::error::Result;
use crate::model::{SenderKind, ToolCallDecision};

/// What `Messenger::act` and `Messenger::stop` hand over. Installed with `install_guard`.
#[async_trait]
pub trait EngineGuard: Send + Sync {
    /// `MessageAction::Decide`, handled here and nowhere else.
    async fn decide(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        decision: ToolCallDecision,
    ) -> Result<()>;

    /// `Conversation.stop`: ends its turns and settles its open calls as cancelled.
    async fn stop_conversation(&self, conversation: ObjectId) -> Result<()>;

    /// May a message from an account of kind `agent` be stored? An error refuses it.
    async fn admit_agent_message(&self, sender: &str) -> Result<()>;
}

/// The kind on a sender's card (ADR 0054). Answers `Unknown` for whom it does not know; never a guess
/// from the message. Implementations must not wait for the guard's own operations: the core calls this
/// while it holds its write lock.
#[async_trait]
pub trait SenderLookup: Send + Sync {
    /// What this sender is.
    async fn sender_kind(&self, sender: &str) -> SenderKind;
}
