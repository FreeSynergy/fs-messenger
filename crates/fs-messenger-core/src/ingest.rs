//! How messages come in: [`Ingest`], and the flow behind it.
//!
//! Pattern: Template method: `IngestSteps::ingest_flow` fixes the order (check -> lock -> dedupe by
//! alias -> store); the store-backed steps fill it in. What a provider says about a tool call never
//! decides anything: the engine's fields are reset here (ADR 0054), and `settled` is final.

use async_trait::async_trait;
use fs_store::ObjectId;
use tokio::sync::OwnedMutexGuard;

use crate::error::{Error, Result};
use crate::model::{ToolCall, Usage};
use crate::provider::RawMessage;

/// What became of a delivered message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingested {
    /// New, stored under this id.
    Stored(ObjectId),
    /// Already known, nothing stored twice.
    Duplicate(ObjectId),
}

/// What one sync did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// New messages.
    pub stored: usize,
    /// Messages that were already known.
    pub duplicates: usize,
}

/// The way in, for whoever runs a provider.
#[async_trait]
pub trait Ingest: Send + Sync {
    /// Take one message from a provider. A replay is recognised by its aliases.
    async fn ingest(&self, account: ObjectId, raw: RawMessage) -> Result<Ingested>;
    /// Fetch from the account's provider since its cursor and ingest everything. The cursor moves
    /// after a batch is stored; a crash in between only causes a replay.
    async fn sync(&self, account: ObjectId) -> Result<SyncReport>;
    /// Store or update one call of an agent, in place, keyed by the agent's own reference. Answers
    /// the call as stored: `id` and `revision` are the engine's; risk, options, digest and `asked_of`
    /// are reset and `detail` moves into `summary`, because the call came from outside the engine.
    /// A settled call stays as it is, and so does a call given a lower non-zero revision than held.
    async fn record_tool_call(
        &self,
        message: ObjectId,
        agent_ref: &str,
        call: ToolCall,
    ) -> Result<ToolCall>;
    /// Replace the usage total of a conversation.
    async fn record_usage(&self, conversation: ObjectId, usage: Usage) -> Result<()>;
}

/// The steps of the flow.
#[async_trait]
pub(crate) trait IngestSteps: Send + Sync {
    /// Dedupe and store must not interleave with another delivery.
    async fn exclusive(&self) -> OwnedMutexGuard<()>;
    /// The stored message one of the aliases points at, if any.
    async fn known(&self, account: ObjectId, raw: &RawMessage) -> Result<Option<ObjectId>>;
    /// Remember aliases not seen so far on a known message.
    async fn merge_aliases(&self, id: ObjectId, account: ObjectId, raw: &RawMessage) -> Result<()>;
    /// Store a new message.
    async fn store_new(&self, account: ObjectId, raw: RawMessage) -> Result<ObjectId>;

    /// The fixed flow.
    async fn ingest_flow(&self, account: ObjectId, raw: RawMessage) -> Result<Ingested> {
        check(&raw)?;
        let _writes = self.exclusive().await;
        if let Some(id) = self.known(account, &raw).await? {
            self.merge_aliases(id, account, &raw).await?;
            return Ok(Ingested::Duplicate(id));
        }
        Ok(Ingested::Stored(self.store_new(account, raw).await?))
    }
}

/// Without an alias a replay cannot be recognised; such a message would be stored twice.
fn check(raw: &RawMessage) -> Result<()> {
    if raw.aliases.is_empty() {
        return Err(Error::Invalid(
            "a provider must give every message at least one alias, creating its own if needed"
                .into(),
        ));
    }
    let bad = |s: &str| s.is_empty() || s.contains('|');
    if raw
        .aliases
        .iter()
        .chain([&raw.conversation])
        .any(|a| bad(&a.scheme) || a.id.is_empty())
    {
        return Err(Error::Invalid(
            "an alias needs an id and a scheme without `|`".into(),
        ));
    }
    Ok(())
}
