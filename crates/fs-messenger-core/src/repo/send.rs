//! Sending: plan, check, record, hand over, record the answer (the records: `send_record.rs`).
//!
//! Pattern: Template method (fixed order) with the check of ADR 0018 first: before it, nothing is
//! written to the messages and no provider has seen the message. The message is stored as `sending`
//! before the provider runs, so a crash shows it instead of losing it.

use fs_objects::ObjectId;

use super::draft_data::DraftData;
use super::handle::Inner;
use crate::error::{Error, Result};
use crate::gate;
use crate::model::EncryptionPlan;
use crate::provider::{Outgoing, Provider};

impl Inner {
    /// What the account's provider plans for `recipients`.
    pub(super) async fn plan_for(
        &self,
        account: Option<ObjectId>,
        recipients: &[String],
    ) -> Result<EncryptionPlan> {
        let provider = account.and_then(|a| self.provider(a));
        plan_with(provider.as_deref(), recipients).await
    }

    /// The one way a message leaves the core. The provider is looked up once: whoever made the plan
    /// is who gets the message, whatever is registered in the meantime.
    pub(super) async fn send_draft(&self, data: &DraftData) -> Result<ObjectId> {
        let recipients = data.recipients();
        let account = data
            .account
            .ok_or_else(|| Error::Invalid("no account chosen; nothing left".into()))?;
        let provider = self.provider(account);
        let plan = plan_with(provider.as_deref(), &recipients).await?;
        gate::check(&plan, &recipients)?;
        let provider = provider.ok_or_else(|| Error::Provider("no provider".into()))?;

        let (message, body) = self.record_sending(data, account, &plan).await?;
        let outgoing = Outgoing::approve(message.id(), plan, body)?;
        let id = message.id();
        match provider.send(&outgoing).await {
            Ok(receipt) => {
                let scope = message.alias_scope.as_str();
                self.record_sent(id, scope, receipt, data).await?;
            }
            Err(e) => {
                self.record_failed(id).await?;
                return Err(e);
            }
        }
        Ok(id)
    }
}

/// A plan, normalised. No provider, or one that cannot plan, is "no way".
async fn plan_with(
    provider: Option<&dyn Provider>,
    recipients: &[String],
) -> Result<EncryptionPlan> {
    let Some(provider) = provider else {
        return Ok(gate::normalise(EncryptionPlan::default(), recipients));
    };
    match provider.encryption_plan(recipients).await {
        Ok(plan) => Ok(gate::normalise(plan, recipients)),
        Err(Error::Unsupported(_)) => Ok(gate::normalise(EncryptionPlan::default(), recipients)),
        Err(e) => Err(e),
    }
}
