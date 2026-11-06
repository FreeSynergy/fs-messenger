//! The store-backed steps of the ingest flow, and the sync loop.
//!
//! Pattern: Template method (the steps; the flow is `IngestSteps::ingest_flow`). The alias is kept
//! on the object's index because `fs-store` has no table of our own; see `DESIGN.md`.

use async_trait::async_trait;
use fs_store::{Cond, ObjectId, Op, Query};
use tokio::sync::OwnedMutexGuard;

use super::consts::{IDX_ALIAS, IDX_LINK, IDX_RECEIPT, NOTE};
use super::handle::Inner;
use super::keys::{alias_key, same_address};
use super::message_codec::{message_from, message_to};
use crate::error::{Error, Result};
use crate::ingest::{IngestSteps, Ingested, SyncReport};
use crate::model::ExternalId;
use crate::provider::RawMessage;

#[async_trait]
impl IngestSteps for Inner {
    fn max_message_bytes(&self) -> usize {
        self.config.max_message_bytes
    }

    async fn exclusive(&self) -> OwnedMutexGuard<()> {
        self.writes.clone().lock_owned().await
    }

    /// Only an id the provider issued makes a message known. The one exception is a copy of what we
    /// sent ourselves: it may match a receipt, but only with the account's own address as sender.
    async fn known(&self, account: ObjectId, raw: &RawMessage) -> Result<Option<ObjectId>> {
        let (_, content) = self.load_account(account).await?;
        let scope = content.scope();
        let eq = |key: &str, alias: &ExternalId| Cond::Index {
            key: key.to_string(),
            op: Op::Eq,
            value: alias_key(&scope, alias),
        };
        let mut q = Query::new().limit(1);
        for alias in &raw.aliases {
            q = q.or(eq(IDX_ALIAS, alias));
        }
        if same_address(&raw.sender, &content.address) {
            for alias in raw.aliases.iter().chain(&raw.links) {
                q = q.or(eq(IDX_RECEIPT, alias));
            }
        }
        Ok(self.store.query(&q).await?.pop().map(|o| o.envelope.id))
    }

    async fn merge_aliases(&self, id: ObjectId, account: ObjectId, raw: &RawMessage) -> Result<()> {
        let object = self.store.get(id).await?.ok_or(Error::NotFound(id))?;
        if object.envelope.object_type == NOTE {
            let mut m = message_from(object)?;
            let before = (m.aliases.len(), m.links.len());
            add_new(&mut m.aliases, &raw.aliases);
            add_new(&mut m.links, &raw.links);
            if before != (m.aliases.len(), m.links.len()) {
                self.put(message_to(&m)?).await?;
            }
            return Ok(());
        }
        let (_, content) = self.load_account(account).await?;
        let scope = content.scope();
        let mut tombstone = object;
        let mut changed = false;
        for (key, ids) in [(IDX_ALIAS, &raw.aliases), (IDX_LINK, &raw.links)] {
            let held = tombstone.index.entry(key.to_string()).or_default();
            for alias in ids {
                let value = alias_key(&scope, alias);
                if !held.contains(&value) {
                    held.push(value);
                    changed = true;
                }
            }
        }
        tombstone.index.retain(|_, values| !values.is_empty());
        if changed {
            self.put(tombstone).await?;
        }
        Ok(())
    }

    async fn store_new(&self, account: ObjectId, raw: RawMessage) -> Result<ObjectId> {
        self.store_raw(account, raw).await
    }
}

fn add_new(held: &mut Vec<ExternalId>, new: &[ExternalId]) {
    for alias in new {
        if !held.contains(alias) {
            held.push(alias.clone());
        }
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
                    Err(Error::Invalid(_) | Error::TooLarge { .. }) => report.rejected += 1,
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
