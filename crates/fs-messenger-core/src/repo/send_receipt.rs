//! What a provider's answer changes: state, what the message is known as, and its conversation.
//!
//! Pattern: State (`sending` -> `sent`). Runs after the message left, so nothing here can undo that;
//! what the provider reports is recorded as it is, also on a tombstone, so a copy does not revive a
//! message deleted on its way out.

use fs_store::{ObjectId, Query};

use super::consts::{IDX_RECEIPT, NOTE};
use super::handle::Inner;
use super::keys::alias_key;
use super::message_codec::message_to;
use crate::error::{Error, Result};
use crate::model::{DeliveryState, ExternalId};
use crate::provider::SendReceipt;

impl Inner {
    pub(super) async fn record_sent(
        &self,
        id: ObjectId,
        scope: &str,
        receipt: SendReceipt,
        draft: ObjectId,
    ) -> Result<()> {
        let actual = receipt.encryption;
        {
            let _writes = self.writes.lock().await;
            if let Some(mut message) = self.reload(id).await? {
                let context = message.summary.envelope.context.as_deref();
                let provisional: ObjectId = context.unwrap_or_default().parse()?;
                let conversation = self
                    .settle_conversation(provisional, scope, receipt.conversation)
                    .await?;
                message.summary.envelope.context = Some(conversation.to_string());
                message.summary.state = DeliveryState::Sent;
                message.summary.encryption = actual;
                message.receipts = receipt.aliases;
                self.put(message_to(&message)?).await?;
                self.drop_if_empty(provisional, conversation).await?;
                self.refresh(conversation).await?;
            } else {
                self.remember_on_tombstone(id, scope, &receipt.aliases)
                    .await?;
            }
        }
        self.drop_draft_object(draft).await?;
        if actual.is_end_to_end() {
            Ok(())
        } else {
            Err(Error::EncryptionDowngraded {
                message: id,
                encryption: actual,
            })
        }
    }

    /// A message deleted while it was on its way must not come back as its own copy.
    async fn remember_on_tombstone(
        &self,
        id: ObjectId,
        scope: &str,
        aliases: &[ExternalId],
    ) -> Result<()> {
        let Some(mut tombstone) = self.store.get(id).await? else {
            return Ok(());
        };
        let held = tombstone.index.entry(IDX_RECEIPT.to_string()).or_default();
        held.extend(aliases.iter().map(|a| alias_key(scope, a)));
        tombstone.index.retain(|_, values| !values.is_empty());
        self.put(tombstone).await
    }

    /// A conversation learns what the provider calls it, so that replies land in the same place. A
    /// new one whose thread is already known gives way to the one that exists.
    async fn settle_conversation(
        &self,
        conversation: ObjectId,
        scope: &str,
        alias: Option<ExternalId>,
    ) -> Result<ObjectId> {
        let Some(alias) = alias else {
            return Ok(conversation);
        };
        let mut record = self.load_conversation(conversation).await?;
        if record.content.aliases.contains(&alias) {
            return Ok(conversation);
        }
        if let Some(known) = self.find_conversation(scope, &alias).await? {
            let is_new = record.content.aliases.is_empty();
            return Ok(if is_new {
                known.envelope.id
            } else {
                conversation
            });
        }
        record.content.aliases.push(alias);
        self.put(record.to_object()?).await?;
        Ok(conversation)
    }

    /// The new conversation a message has moved out of, if nothing else is in it.
    async fn drop_if_empty(&self, provisional: ObjectId, kept: ObjectId) -> Result<()> {
        if provisional == kept {
            return Ok(());
        }
        let notes = Query::new()
            .of_type(NOTE)
            .in_context(provisional.to_string());
        if self.store.count(&notes).await? == 0 {
            self.store.delete(provisional, &self.config.actor).await?;
        }
        Ok(())
    }
}
