//! Storing a new message from a provider.
//!
//! Pattern: Adapter: raw message to `Note` envelope + content + original (ADR 0030). Encryption is
//! taken as delivered, and an unset one stays `unknown`: incoming unencrypted is stored and marked,
//! never held back (ADR 0018). Outgoing rules do not apply here.

use std::collections::BTreeMap;

use chrono::Utc;
use fs_store::{Envelope, ObjectId};

use super::consts::NOTE;
use super::handle::Inner;
use super::keys::same_address;
use super::message_codec::message_to;
use super::text::{body_media_type, display_name, preview};
use super::tool_calls;
use crate::error::Result;
use crate::model::{DeliveryState, Message, MessageSummary, SenderKind, StoredForm};
use crate::provider::RawMessage;

impl Inner {
    pub(super) async fn store_raw(
        &self,
        account: ObjectId,
        raw: RawMessage,
        too_large: Option<u64>,
    ) -> Result<ObjectId> {
        let name = raw
            .conversation_name
            .as_deref()
            .or(raw.subject.as_deref())
            .unwrap_or(raw.sender.as_str());
        let (_, owner) = self.load_account(account).await?;
        let scope = owner.scope();
        let conversation = self
            .ensure_conversation(account, &scope, &raw.conversation, name, raw.kind)
            .await?;
        let conversation_id = conversation.envelope.id;
        // A message cannot answer itself; the mail stays, the claim goes.
        let parent = raw
            .in_reply_to
            .clone()
            .filter(|p| !raw.aliases.contains(p) && !raw.links.contains(p));
        let (in_reply_to, pending_reply) = self.resolve_parent(&scope, parent).await?;
        // Only the account's own address can write as me: a stranger's claim is an ordinary mail.
        let outgoing = raw.outgoing && same_address(&raw.sender, &owner.address);

        // Ciphertext is no name and no preview.
        let shown = if raw.undecrypted {
            ""
        } else {
            raw.body_text.as_str()
        };
        let mut env = Envelope::new(NOTE, display_name(raw.subject.as_deref(), shown));
        let preview = preview(shown);
        env.iri = self.iri(env.id);
        env.attributed_to = (!raw.sender.is_empty()).then(|| raw.sender.clone());
        env.to = raw.to;
        env.cc = raw.cc;
        env.context = Some(conversation_id.to_string());
        env.in_reply_to = in_reply_to.map(|id| id.to_string());
        let arrived = Utc::now();
        env.published = raw.published.unwrap_or(arrived);
        // A date in the future shows, but must not hold the top of the list.
        let sort_at = (env.published > arrived).then_some(arrived);

        let mut attachments = Vec::with_capacity(raw.attachments.len());
        for a in raw.attachments {
            attachments.push(self.store_attachment(a.name, a.media_type, a.data).await?);
        }
        let mut message = Message {
            summary: MessageSummary {
                envelope: env,
                kind: raw.kind,
                encryption: raw.encryption,
                state: if outgoing {
                    DeliveryState::Sent
                } else {
                    DeliveryState::Delivered
                },
                unread: !outgoing,
                flagged: false,
                preview,
                // Nothing here can look at the sender's card yet, and a remote must not claim `person`.
                sender_kind: SenderKind::Unknown,
                job_ref: raw.job_ref,
                arrival: self.next_arrival(),
                undecrypted: raw.undecrypted,
                signature: raw.signature,
                too_large: too_large.unwrap_or(0),
            },
            body_text: raw.body_text,
            body_html: String::new(),
            attachments,
            reactions: vec![],
            original_media_type: raw
                .original
                .as_ref()
                .map(|b| b.media_type.clone())
                .unwrap_or_default(),
            original: raw.original,
            tool_calls: vec![],
            tool_call_refs: BTreeMap::new(),
            task_refs: raw.task_refs,
            reasoning: raw.reasoning,
            body_media_type: body_media_type(&raw.body_media_type),
            aliases: raw.aliases,
            links: raw.links,
            receipts: vec![],
            alias_scope: scope.clone(),
            account: Some(account),
            outgoing,
            pending_reply,
            references: raw.references,
            sort_at,
            stored: StoredForm::default(),
        };
        for t in raw.tool_calls {
            tool_calls::apply(&mut message, &t.agent_ref, t.call);
        }
        let id = message.id();
        self.externalise_body(&mut message).await?;
        self.put(message_to(&message)?).await?;
        self.adopt_children(&scope, &message).await?;
        self.refresh(conversation_id).await?;
        Ok(id)
    }
}
