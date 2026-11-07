//! What the send flow records: `sending` before the provider runs, then its answer.
//!
//! Pattern: State: `sending` -> `sent` or `failed`. A provider's answer is reloaded against the store
//! under the write lock, so a change made while the message was on its way is kept.

use fs_objects::{Envelope, ObjectId};

use super::consts::NOTE;
use super::draft_data::DraftData;
use super::handle::Inner;
use super::message_codec::message_to;
use super::text::{display_name, preview};
use crate::error::{Error, Result};
use crate::ingest_rules::printable;
use crate::model::{
    DeliveryState, EncryptionPlan, ExternalId, Kind, Message, MessageSummary, SenderKind,
    SignatureCheck, StoredForm,
};
use crate::provider::OutgoingBody;

impl Inner {
    pub(super) async fn record_sending(
        &self,
        data: &DraftData,
        account: ObjectId,
        plan: &EncryptionPlan,
    ) -> Result<(Message, OutgoingBody)> {
        let _writes = self.writes.lock().await;
        let (_, sender) = self.load_account(account).await?;
        let scope = sender.scope();
        let parent = match data.reply_to {
            Some(id) => self.reload(id).await?,
            None => None,
        };
        let (references, chain, in_reply_to) = parent.map_or_else(Default::default, |m| {
            let mut chain = m.references.clone();
            chain.extend(m.links.iter().chain(&m.receipts).cloned());
            (
                m.references,
                chain,
                [m.aliases, m.links, m.receipts].concat(),
            )
        });
        // Nothing that was stored before this rule existed may reach a provider either.
        let clean = |mut ids: Vec<ExternalId>| {
            ids.retain(ExternalId::is_well_formed);
            ids
        };
        let (references, in_reply_to) = (clean(references), clean(in_reply_to));
        // Before anything is stored: without the files there is nothing to send.
        let attachments = self.outgoing_attachments(&data.attachments).await?;
        let conversation = if let Some(id) = data.conversation {
            self.load_conversation(id).await?
        } else {
            let kind = if sender.kind == "mail" {
                Kind::Mail
            } else {
                Kind::Chat
            };
            // Never the hidden recipients: a name is shown.
            let name = [data.to.as_slice(), &data.cc].concat().join(", ");
            self.create_conversation(account, &scope, vec![], &name, kind)
                .await?
        };
        let conversation_id = conversation.envelope.id;
        let mut env = Envelope::new(NOTE, display_name(Some(&data.subject), &data.text));
        env.iri = self.iri(env.id);
        env.attributed_to = Some(sender.address);
        env.to.clone_from(&data.to);
        env.cc.clone_from(&data.cc);
        env.bcc.clone_from(&data.bcc);
        env.context = Some(conversation_id.to_string());
        env.in_reply_to = data.reply_to.map(|id| id.to_string());
        let mut message = Message {
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
                arrival: self.next_arrival(),
                undecrypted: false,
                signature: SignatureCheck::default(),
                too_large: 0,
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
            links: vec![],
            receipts: vec![],
            alias_scope: scope,
            account: Some(account),
            outgoing: true,
            pending_reply: None,
            references: chain,
            sort_at: None,
            stored: StoredForm::default(),
        };
        self.externalise_body(&mut message).await?;
        self.put(message_to(&message)?).await?;
        self.refresh(conversation_id).await?;
        let body = OutgoingBody {
            to: data.to.clone(),
            cc: data.cc.clone(),
            bcc: data.bcc.clone(),
            text: data.text.clone(),
            media_type: data.media_type.clone(),
            attachments,
            conversation: clean(conversation.content.aliases),
            in_reply_to,
            references,
            subject: printable(&data.subject),
        };
        Ok((message, body))
    }

    /// A message that was deleted in the meantime stays deleted.
    pub(super) async fn reload(&self, id: ObjectId) -> Result<Option<Message>> {
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
}
