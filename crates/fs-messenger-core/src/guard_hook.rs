//! What the core asks of a guard: the decisions and stops it routes, and who a sender is.
//!
//! Pattern: Strategy: the core depends on these traits and never on the crate `fs-messenger-guard`,
//! which depends on the core. A `Provider` never sees them. Without an installed guard the core answers
//! `Unsupported` for `decide` and `stop`, which is the safe default.

use async_trait::async_trait;
use fs_objects::ObjectId;

use crate::error::Result;
use crate::model::{SenderKind, ToolCallDecision};

/// Why a decision was refused: `Problem.type` of the contract (draft 0.4). On `Stale` and `Settled` a
/// `toolCall` event with the state as it really is follows, so a card can show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionProblem {
    /// Digest or revision no longer match.
    Stale,
    /// The call is settled.
    Settled,
    /// The signer is not the person asked.
    NotAskedOf,
    /// Empty field, unknown option; bad, replayed or expired proof.
    Invalid,
}

impl DecisionProblem {
    /// The URI for `Problem.type`.
    pub fn type_uri(self) -> &'static str {
        match self {
            Self::Stale => "https://freesynergy.net/problems/decision-stale",
            Self::Settled => "https://freesynergy.net/problems/decision-settled",
            Self::NotAskedOf => "https://freesynergy.net/problems/decision-not-asked-of-you",
            Self::Invalid => "https://freesynergy.net/problems/decision-invalid",
        }
    }
}

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
