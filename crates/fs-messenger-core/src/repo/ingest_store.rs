//! Storing a new message from a provider.
//!
//! Pattern: Adapter: raw message to `Note` envelope + content + original (ADR 0030). Encryption is
//! taken as delivered, and an unset one stays `unknown`: incoming unencrypted is stored and marked,
//! never held back (ADR 0018). Outgoing rules do not apply here.

use std::collections::BTreeMap;

use chrono::Utc;
use fs_store::{Envelope, ObjectId, Query};

use super::consts::{IDX_ALIAS, IDX_REPLY_ALIAS, NOTE};
use super::handle::Inner;
use super::keys::alias_key;
use super::message_codec::{message_from, message_to};
use super::text::{body_media_type, display_name, preview};
use super::tool_calls;
use crate::error::Result;
use crate::model::{DeliveryState, ExternalId, Message, MessageSummary, SenderKind};
use crate::provider::RawMessage;

const MAX_CHAIN: usize = 1000;

impl Inner {
    pub(super) async fn store_raw(&self, account: ObjectId, raw: RawMessage) -> Result<ObjectId> {
        let name = raw
            .conversation_name
            .as_deref()
            .or(raw.subject.as_deref())
            .unwrap_or(raw.sender.as_str());
        let conversation = self
            .ensure_conversation(account, &raw.conversation, name, raw.kind)
            .await?;
        let conversation_id = conversation.envelope.id;
        // A message cannot answer itself; the mail stays, the claim goes.
        let parent = raw.in_reply_to.clone().filter(|p| !raw.aliases.contains(p));
        let (in_reply_to, pending_reply) = self.resolve_parent(account, parent).await?;

        let mut env = Envelope::new(NOTE, display_name(raw.subject.as_deref(), &raw.body_text));
        env.iri = self.iri(env.id);
        env.attributed_to = (!raw.sender.is_empty()).then(|| raw.sender.clone());
        env.to = raw.to;
        env.cc = raw.cc;
        env.context = Some(conversation_id.to_string());
        env.in_reply_to = in_reply_to.map(|id| id.to_string());
        env.published = raw.published.unwrap_or_else(Utc::now);

        let mut attachments = Vec::with_capacity(raw.attachments.len());
        for a in raw.attachments {
            attachments.push(self.store_attachment(a.name, a.media_type, a.data).await?);
        }
        let mut message = Message {
            summary: MessageSummary {
                envelope: env,
                kind: raw.kind,
                encryption: raw.encryption,
                state: if raw.outgoing {
                    DeliveryState::Sent
                } else {
                    DeliveryState::Delivered
                },
                unread: !raw.outgoing,
                flagged: false,
                preview: preview(&raw.body_text),
                // Nothing here can look at the sender's card yet, and a remote must not claim `person`.
                sender_kind: SenderKind::Unknown,
                job_ref: raw.job_ref,
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
            account: Some(account),
            outgoing: raw.outgoing,
            pending_reply,
        };
        for t in raw.tool_calls {
            tool_calls::apply(&mut message, &t.agent_ref, t.call);
        }
        let id = message.id();
        self.put(message_to(&message)?).await?;
        self.adopt_children(account, &message).await?;
        self.refresh(conversation_id).await?;
        Ok(id)
    }

    /// Our id of the parent if it is here; otherwise remember what the provider calls it.
    async fn resolve_parent(
        &self,
        account: ObjectId,
        parent: Option<ExternalId>,
    ) -> Result<(Option<ObjectId>, Option<ExternalId>)> {
        let Some(parent) = parent else {
            return Ok((None, None));
        };
        let q = Query::new()
            .with_index(IDX_ALIAS, alias_key(account, &parent))
            .limit(1);
        Ok(match self.store.query(&q).await?.pop() {
            Some(o) => (Some(o.envelope.id), None),
            None => (None, Some(parent)),
        })
    }

    /// Replies that arrived before their parent now learn its id; a re-sync can come in any order.
    /// A link that would close a cycle is given up: the reply stays in the conversation, unlinked.
    async fn adopt_children(&self, account: ObjectId, parent: &Message) -> Result<()> {
        for alias in &parent.aliases {
            let q = Query::new()
                .of_type(NOTE)
                .with_index(IDX_REPLY_ALIAS, alias_key(account, alias));
            for o in self.store.query(&q).await? {
                let mut child = message_from(o)?;
                child.pending_reply = None;
                if !self.is_above(child.id(), parent).await? {
                    child.summary.envelope.in_reply_to = Some(parent.id().to_string());
                }
                self.put(message_to(&child)?).await?;
            }
        }
        Ok(())
    }

    /// Whether `ancestor` is `from` or above it in the reply chain. A chain longer than anyone
    /// writes counts as yes.
    async fn is_above(&self, ancestor: ObjectId, from: &Message) -> Result<bool> {
        let mut at = from.summary.envelope.in_reply_to.clone();
        for _ in 0..MAX_CHAIN {
            let Some(id) = at.and_then(|s| s.parse::<ObjectId>().ok()) else {
                return Ok(false);
            };
            if id == ancestor {
                return Ok(true);
            }
            let Some(m) = self.reload(id).await? else {
                return Ok(false);
            };
            at = m.summary.envelope.in_reply_to;
        }
        Ok(true)
    }
}
