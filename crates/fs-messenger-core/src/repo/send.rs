//! Sending: plan, check, record, hand over, record the answer (the records: `send_record.rs`).
//!
//! Pattern: Template method (fixed order) with the check of ADR 0018 first: before it, nothing is
//! written to the messages and no provider has seen the message. The message is stored as `sending`
//! before the provider runs, so a crash shows it instead of losing it.

use fs_store::ObjectId;

use super::draft_data::DraftData;
use super::handle::Inner;
use crate::error::{Error, Result};
use crate::gate;
use crate::model::EncryptionPlan;
use crate::provider::{Outgoing, Provider};

impl Inner {
    /// What the provider plans, normalised. No provider, or one that cannot plan, is "no way".
    pub(super) async fn plan_for(
        &self,
        account: Option<ObjectId>,
        recipients: &[String],
    ) -> Result<EncryptionPlan> {
        let Some(provider) = account.and_then(|a| self.provider(a)) else {
            return Ok(gate::normalise(EncryptionPlan::default(), recipients));
        };
        match provider.encryption_plan(recipients).await {
            Ok(plan) => Ok(gate::normalise(plan, recipients)),
            Err(Error::Unsupported(_)) => {
                Ok(gate::normalise(EncryptionPlan::default(), recipients))
            }
            Err(e) => Err(e),
        }
    }

    /// The one way a message leaves the core.
    pub(super) async fn send_draft(&self, data: &DraftData) -> Result<ObjectId> {
        let recipients = data.recipients();
        let account = data
            .account
            .ok_or_else(|| Error::Invalid("no account chosen; nothing left".into()))?;
        let plan = self.plan_for(Some(account), &recipients).await?;
        gate::check(&plan, &recipients)?;
        let provider: std::sync::Arc<dyn Provider> = self
            .provider(account)
            .ok_or_else(|| Error::Provider("the provider is gone".into()))?;

        let (message, body) = self.record_sending(data, account, &plan).await?;
        let outgoing = Outgoing::approve(message.id(), plan, body)?;
        let id = message.id();
        match provider.send(&outgoing).await {
            Ok(receipt) => self.record_sent(id, receipt, data.id).await?,
            Err(e) => {
                self.record_failed(id).await?;
                return Err(e);
            }
        }
        Ok(id)
    }
}
