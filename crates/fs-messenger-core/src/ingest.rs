//! How messages come in: [`Ingest`], and the flow behind it.
//!
//! Pattern: Template method: `IngestSteps::ingest_flow` fixes the order (rules -> lock -> dedupe by
//! alias -> store); the store-backed steps fill it in. What a provider says about a tool call never
//! decides anything: the engine's fields are reset here (ADR 0054), and `settled` is final.

use async_trait::async_trait;
use fs_store::ObjectId;
use tokio::sync::OwnedMutexGuard;

use crate::error::Result;
use crate::ingest_rules;
use crate::model::{SenderKind, ToolCall, Usage};
use crate::provider::RawMessage;

/// What became of a delivered message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingested {
    /// New, stored under this id.
    Stored(ObjectId),
    /// Already known, nothing stored twice.
    Duplicate(ObjectId),
    /// Over the limit: only a stub stays, with who, when and how large, so it is visible, not gone.
    TooLarge {
        /// The stub.
        id: ObjectId,
        /// The size of the message, by the rule in `DESIGN.md`.
        size: usize,
        /// The limit it broke; a message with too many files can be under it.
        limit: usize,
    },
}

/// What one sync did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// New messages.
    pub stored: usize,
    /// Messages that were already known.
    pub duplicates: usize,
    /// Messages refused for what they are (invalid, too large, or a local record in the way). They
    /// do not stop the sync, and the cursor goes past them. One too large leaves a stub.
    pub rejected: usize,
}

/// The way in, for whoever runs a provider.
#[async_trait]
pub trait Ingest: Send + Sync {
    /// Take one message from a provider. A replay is recognised by an alias the provider issued; a
    /// copy of what we sent by its receipt, if its sender is the account's own address.
    async fn ingest(&self, account: ObjectId, raw: RawMessage) -> Result<Ingested>;
    /// Fetch from the account's provider since its cursor and ingest everything. The cursor moves
    /// after a batch is stored; a crash in between only causes a replay.
    async fn sync(&self, account: ObjectId) -> Result<SyncReport>;
    /// Store or update one call of an agent, in place, keyed by the agent's own reference. Answers
    /// the call as stored: `id` and `revision` are the engine's; risk, options, digest and `asked_of`
    /// are reset and `detail` moves into `summary`, because the call came from outside the engine.
    /// An outside question is stored as `working`: nobody here can answer it. An incoming revision
    /// is ignored, and a settled call stays as it is. An empty `agent_ref` is refused.
    async fn record_tool_call(
        &self,
        message: ObjectId,
        agent_ref: &str,
        call: ToolCall,
    ) -> Result<ToolCall>;
    /// Replace the usage total of a conversation.
    async fn record_usage(&self, conversation: ObjectId, usage: Usage) -> Result<()>;
}

/// What a delivered message matched.
pub(crate) struct Known {
    /// The stored message.
    pub(crate) id: ObjectId,
    /// By an id the provider issued. A match on a receipt only says "this is my own copy".
    pub(crate) by_issuer: bool,
}

/// The steps of the flow.
#[async_trait]
pub(crate) trait IngestSteps: Send + Sync {
    /// The largest message taken.
    fn max_message_bytes(&self) -> usize;
    /// What this sender is, and whether it may come in at all; before the lock, and for a known
    /// message too: a repeat must not get in what a first delivery could not.
    async fn admit(&self, account: ObjectId, raw: &RawMessage) -> Result<SenderKind>;
    /// Dedupe and store must not interleave with another delivery.
    async fn exclusive(&self) -> OwnedMutexGuard<()>;
    /// The stored message this delivery is, if any.
    async fn known(&self, account: ObjectId, raw: &RawMessage) -> Result<Option<Known>>;
    /// Remember what a known message is also called.
    async fn merge_aliases(&self, id: ObjectId, account: ObjectId, raw: &RawMessage) -> Result<()>;
    /// A known message that could not be decrypted gets the readable version.
    async fn upgrade(&self, id: ObjectId, raw: RawMessage) -> Result<()>;
    /// Store a new message; with a size, only what stays of one too large.
    async fn store_new(
        &self,
        account: ObjectId,
        raw: RawMessage,
        too_large: Option<usize>,
        sender_kind: SenderKind,
    ) -> Result<ObjectId>;

    /// The fixed flow.
    async fn ingest_flow(&self, account: ObjectId, raw: RawMessage) -> Result<Ingested> {
        let limit = self.max_message_bytes();
        let (raw, too_large) = ingest_rules::judged(raw, limit)?;
        let sender_kind = self.admit(account, &raw).await?;
        let _writes = self.exclusive().await;
        if let Some(known) = self.known(account, &raw).await? {
            if known.by_issuer {
                self.merge_aliases(known.id, account, &raw).await?;
                if too_large.is_none() {
                    self.upgrade(known.id, raw).await?;
                }
            }
            return Ok(Ingested::Duplicate(known.id));
        }
        let id = self.store_new(account, raw, too_large, sender_kind).await?;
        Ok(match too_large {
            Some(size) => Ingested::TooLarge { id, size, limit },
            None => Ingested::Stored(id),
        })
    }
}
