//! The store-backed steps of the ingest flow, and the sync loop.
//!
//! Pattern: Template method (the steps; the flow is `IngestSteps::ingest_flow`). The alias is kept
//! on the object's index because `fs-store` has no table of our own; see `DESIGN.md`.

use async_trait::async_trait;
use fs_store::{Cond, ObjectId, Op, Query};
use tokio::sync::OwnedMutexGuard;

use super::consts::{IDX_ALIAS, NOTE};
use super::handle::Inner;
use super::keys::alias_key;
use super::message_codec::{message_from, message_to};
use crate::error::{Error, Result};
use crate::ingest::{IngestSteps, Ingested, SyncReport};
use crate::provider::RawMessage;

#[async_trait]
impl IngestSteps for Inner {
    async fn exclusive(&self) -> OwnedMutexGuard<()> {
        self.writes.clone().lock_owned().await
    }

    async fn known(&self, account: ObjectId, raw: &RawMessage) -> Result<Option<ObjectId>> {
        self.load_account(account).await?;
        let mut q = Query::new().limit(1);
        for alias in &raw.aliases {
            q = q.or(Cond::Index {
                key: IDX_ALIAS.to_string(),
                op: Op::Eq,
                value: alias_key(account, alias),
            });
        }
        Ok(self.store.query(&q).await?.pop().map(|o| o.envelope.id))
    }

    async fn merge_aliases(&self, id: ObjectId, account: ObjectId, raw: &RawMessage) -> Result<()> {
        let mut object = self.store.get(id).await?.ok_or(Error::NotFound(id))?;
        if object.envelope.object_type == NOTE {
            let mut m = message_from(object)?;
            let new: Vec<_> = raw
                .aliases
                .iter()
                .filter(|a| !m.aliases.contains(a))
                .cloned()
                .collect();
            if !new.is_empty() {
                m.aliases.extend(new);
                self.put(message_to(&m)?).await?;
            }
            return Ok(());
        }
        let held = object.index.entry(IDX_ALIAS.to_string()).or_default();
        let before = held.len();
        for alias in &raw.aliases {
            let key = alias_key(account, alias);
            if !held.contains(&key) {
                held.push(key);
            }
        }
        if held.len() != before {
            self.put(object).await?;
        }
        Ok(())
    }

    async fn store_new(&self, account: ObjectId, raw: RawMessage) -> Result<ObjectId> {
        self.store_raw(account, raw).await
    }
}

impl Inner {
    /// Fetch and ingest until the provider has no more. The cursor moves after each stored batch.
    /// One bad message does not hold an account back; a store failure stops here, cursor unmoved.
    pub(super) async fn sync(&self, account: ObjectId) -> Result<SyncReport> {
        let (_, mut content) = self.load_account(account).await?;
        let provider = self
            .provider(account)
            .ok_or_else(|| Error::Provider(format!("no provider for account {account}")))?;
        let mut report = SyncReport::default();
        loop {
            let batch = provider.fetch(content.sync_cursor.as_ref()).await?;
            for raw in batch.messages {
                match self.ingest_flow(account, raw).await {
                    Ok(Ingested::Stored(_)) => report.stored += 1,
                    Ok(Ingested::Duplicate(_)) => report.duplicates += 1,
                    Err(Error::Invalid(_)) => report.rejected += 1,
                    Err(e) => return Err(e),
                }
            }
            let progress = batch.next.is_some() && batch.next != content.sync_cursor;
            if let Some(next) = batch.next.filter(|_| progress) {
                self.save_cursor(account, next.clone()).await?;
                content.sync_cursor = Some(next);
            }
            // `more` without a new position would ask for the same batch for ever.
            if !batch.more || !progress {
                return Ok(report);
            }
        }
    }
}
