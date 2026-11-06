//! What the send flow records: `sending` before the provider runs, then its answer.
//!
//! Pattern: State: `sending` -> `sent` or `failed`. A provider's answer is reloaded against the store
//! under the write lock, so a change made while the message was on its way is kept.

use fs_store::{Envelope, ObjectId};

use super::consts::NOTE;
use super::draft_data::DraftData;
use super::handle::Inner;
use super::message_codec::message_to;
use super::text::{display_name, preview};
use crate::error::{Error, Result};
use crate::model::{
    DeliveryState, EncryptionPlan, ExternalId, Kind, Message, MessageSummary, SenderKind,
};
use crate::provider::{OutgoingBody, SendReceipt};

impl Inner {
    pub(super) async fn record_sending(
        &self,
        data: &DraftData,
        account: ObjectId,
        plan: &EncryptionPlan,
    ) -> Result<(Message, OutgoingBody)> {
        let _writes = self.writes.lock().await;
        let (_, sender) = self.load_account(account).await?;
        let conversation = if let Some(id) = data.conversation {
            self.load_conversation(id).await?
        } else {
            let kind = if sender.kind == "mail" {
                Kind::Mail
            } else {
                Kind::Chat
            };
            let name = data.recipients().join(", ");
            self.create_conversation(account, vec![], &name, kind)
                .await?
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
                sender_kind: SenderKind::Unknown,
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

    /// A message deleted while it was on its way stays deleted.
    async fn reload(&self, id: ObjectId) -> Result<Option<Message>> {
        match self.load_note(id).await {
            Ok(m) => Ok(Some(m)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) async fn record_failed(&self, id: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let Some(mut message) = self.reload(id).await? else {
            return Ok(());
        };
        message.summary.state = DeliveryState::Failed;
        self.put(message_to(&message)?).await
    }

    pub(super) async fn record_sent(
        &self,
        id: ObjectId,
        receipt: SendReceipt,
        draft: ObjectId,
    ) -> Result<()> {
        let actual = receipt.encryption;
        {
            let _writes = self.writes.lock().await;
            if let Some(mut message) = self.reload(id).await? {
                message.summary.state = DeliveryState::Sent;
                message.summary.encryption = actual;
                message.aliases = receipt.aliases;
                self.put(message_to(&message)?).await?;
                let context = message.summary.envelope.context.as_deref();
                let conversation: ObjectId = context.unwrap_or_default().parse()?;
                self.adopt_alias(conversation, receipt.conversation).await?;
                self.refresh(conversation).await?;
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

    /// A conversation learns what the provider calls it, so its replies land in the same place.
    async fn adopt_alias(&self, conversation: ObjectId, alias: Option<ExternalId>) -> Result<()> {
        let Some(alias) = alias else { return Ok(()) };
        let mut record = self.load_conversation(conversation).await?;
        if !record.content.aliases.contains(&alias) {
            record.content.aliases.push(alias);
            self.put(record.to_object()?).await?;
        }
        Ok(())
    }
}
