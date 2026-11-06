//! Sending: plan, check, record, hand over, record the answer.
//!
//! Pattern: Template method (fixed order) with the check of ADR 0018 first: before it, nothing is
//! written to the messages and no provider has seen the message. The message is stored as `sending`
//! before the provider runs, so a crash shows it instead of losing it.

use fs_store::{Envelope, ObjectId};

use super::consts::NOTE;
use super::draft_data::DraftData;
use super::handle::Inner;
use super::message_codec::message_to;
use super::text::{display_name, preview};
use crate::error::{Error, Result};
use crate::gate;
use crate::model::{
    DeliveryState, EncryptionPlan, ExternalId, Kind, Message, MessageSummary, SenderKind,
};
use crate::provider::{Outgoing, OutgoingBody, Provider, SendReceipt};

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
        let answer = provider.send(&outgoing).await;
        let id = message.id();
        match answer {
            Ok(receipt) => self.record_sent(message, account, receipt, data.id).await?,
            Err(e) => {
                self.record_failed(message).await?;
                return Err(e);
            }
        }
        Ok(id)
    }

    async fn record_sending(
        &self,
        data: &DraftData,
        account: ObjectId,
        plan: &EncryptionPlan,
    ) -> Result<(Message, OutgoingBody)> {
        let _writes = self.writes.lock().await;
        let (_, sender) = self.load_account(account).await?;
        let conversation = match data.conversation {
            Some(id) => self.load_conversation(id).await?,
            None => {
                let kind = if sender.kind == "mail" {
                    Kind::Mail
                } else {
                    Kind::Chat
                };
                let name = data.recipients().join(", ");
                self.create_conversation(account, vec![], &name, kind)
                    .await?
            }
        };
        let conversation_id = conversation.envelope.id;
        let mut env = Envelope::new(NOTE, display_name(None, &data.text));
        env.iri = self.iri(env.id);
        env.attributed_to = Some(sender.address);
        env.to.clone_from(&data.to);
        env.cc.clone_from(&data.cc);
        env.bcc.clone_from(&data.bcc);
        env.context = Some(conversation_id.to_string());
        env.in_reply_to = data.reply_to.map(|id| id.to_string());
        let parent = match data.reply_to {
            Some(id) => self.load_note(id).await?.aliases,
            None => vec![],
        };
        let message = Message {
            summary: MessageSummary {
                envelope: env,
                kind: conversation.content.kind,
                encryption: plan.encryption,
                state: DeliveryState::Sending,
                unread: false,
                flagged: false,
                preview: preview(&data.text),
                sender_kind: SenderKind::Person,
                job_ref: String::new(),
            },
            body_text: data.text.clone(),
            body_html: String::new(),
            attachments: data.attachments.clone(),
            reactions: vec![],
            original: None,
            original_media_type: String::new(),
            tool_calls: vec![],
            tool_call_refs: std::collections::BTreeMap::new(),
            task_refs: vec![],
            reasoning: String::new(),
            body_media_type: data.media_type.clone(),
            aliases: vec![],
            account: Some(account),
            outgoing: true,
            pending_reply: None,
        };
        self.put(message_to(&message)?).await?;
        self.refresh(conversation_id).await?;
        let body = OutgoingBody {
            to: data.to.clone(),
            cc: data.cc.clone(),
            bcc: data.bcc.clone(),
            text: data.text.clone(),
            media_type: data.media_type.clone(),
            attachments: self.outgoing_attachments(&data.attachments).await?,
            conversation: conversation.content.aliases,
            in_reply_to: parent,
        };
        Ok((message, body))
    }

    async fn record_failed(&self, mut message: Message) -> Result<()> {
        let _writes = self.writes.lock().await;
        message.summary.state = DeliveryState::Failed;
        self.put(message_to(&message)?).await
    }

    async fn record_sent(
        &self,
        mut message: Message,
        account: ObjectId,
        receipt: SendReceipt,
        draft: ObjectId,
    ) -> Result<()> {
        let id = message.id();
        let actual = receipt.encryption;
        {
            let _writes = self.writes.lock().await;
            message.summary.state = DeliveryState::Sent;
            message.summary.encryption = actual;
            message.aliases = receipt.aliases;
            self.put(message_to(&message)?).await?;
            let context = message
                .summary
                .envelope
                .context
                .as_deref()
                .unwrap_or_default();
            let conversation: ObjectId = context.parse().map_err(Error::Store)?;
            self.adopt_alias(conversation, account, receipt.conversation)
                .await?;
            self.refresh(conversation).await?;
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

    /// A conversation learns what the provider calls it, so its replies land in the same place.
    async fn adopt_alias(
        &self,
        conversation: ObjectId,
        account: ObjectId,
        alias: Option<ExternalId>,
    ) -> Result<()> {
        let Some(alias) = alias else { return Ok(()) };
        let mut record = self.load_conversation(conversation).await?;
        if !record.content.aliases.contains(&alias) {
            record.content.aliases.push(alias);
            record.content.account = Some(account);
            self.put(record.to_object()?).await?;
        }
        Ok(())
    }
}
