//! The message that carries a question of the engine.
//!
//! Pattern: Adapter: an engine call becomes a local `Note`, in the conversation it belongs to or in the
//! participant's own guard conversation. It has no alias and no agent reference, so no delivery and
//! no report of a provider can ever name it.

use std::collections::BTreeMap;

use chrono::Utc;
use fs_store::{Envelope, ObjectId};

use super::consts::{CONVERSATION, NOTE, TEXT_PLAIN};
use super::conversation_codec::{ConversationContent, ConversationRecord};
use super::handle::Inner;
use super::message_codec::message_to;
use crate::error::Result;
use crate::model::{
    DeliveryState, Encryption, ExternalId, Kind, Message, MessageSummary, SenderKind,
    SignatureCheck, StoredForm, ToolCall,
};

/// Who the store's log names for what the engine writes itself. Not a person, so a person's own undo
/// never reaches it. Reaching it anyway changes only what is shown: the guard's record decides.
pub(super) const ENGINE_ACTOR: &str = "urn:freesynergy:messenger:guard";
const GUARD_SCOPE: &str = "freesynergy.net:guard";

impl Inner {
    pub(super) async fn guard_conversation(&self, profile: &str) -> Result<ConversationRecord> {
        let alias = ExternalId::new("guard", profile);
        if let Some(found) = self.find_conversation(GUARD_SCOPE, &alias).await? {
            return Ok(found);
        }
        let record = ConversationRecord {
            envelope: Envelope::new(CONVERSATION, format!("AI guard: {profile}")),
            content: ConversationContent {
                scope: GUARD_SCOPE.to_string(),
                kind: Kind::Chat,
                aliases: vec![alias],
                ..ConversationContent::default()
            },
            raw: None,
        };
        self.put(record.to_object()?).await?;
        Ok(record)
    }

    pub(super) async fn engine_note(
        &self,
        record: &ConversationRecord,
        profile: &str,
        call: ToolCall,
    ) -> Result<ObjectId> {
        let mut env = Envelope::new(NOTE, format!("{profile} asks: {}", call.name));
        env.iri = self.iri(env.id);
        env.attributed_to = Some(profile.to_string());
        env.context = Some(record.envelope.id.to_string());
        env.published = Utc::now();
        let id = env.id;
        let message = Message {
            summary: MessageSummary {
                envelope: env,
                kind: Kind::Chat,
                encryption: Encryption::Unknown,
                state: DeliveryState::Delivered,
                unread: true,
                flagged: false,
                preview: String::new(),
                sender_kind: SenderKind::Ai,
                job_ref: String::new(),
                arrival: self.next_arrival(),
                undecrypted: false,
                signature: SignatureCheck::default(),
                too_large: 0,
            },
            body_text: String::new(),
            body_html: String::new(),
            attachments: vec![],
            reactions: vec![],
            original: None,
            original_media_type: String::new(),
            // No agent reference: nothing a provider reports can name this call.
            tool_calls: vec![call],
            tool_call_refs: BTreeMap::new(),
            task_refs: vec![],
            reasoning: String::new(),
            body_media_type: TEXT_PLAIN.to_string(),
            aliases: vec![],
            links: vec![],
            receipts: vec![],
            alias_scope: String::new(),
            account: record.content.account,
            outgoing: false,
            pending_reply: None,
            references: vec![],
            sort_at: None,
            stored: StoredForm::default(),
        };
        self.store.put(message_to(&message)?, ENGINE_ACTOR).await?;
        Ok(id)
    }
}
