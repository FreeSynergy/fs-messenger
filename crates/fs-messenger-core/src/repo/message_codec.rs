//! Message <-> stored object: AS2 `Note` envelope + content + index fields + immutable original.
//!
//! Pattern: Adapter: maps the [`Message`] view onto what `fs-store` holds (ADR 0020, 0030). Every
//! field of the content has a fail-safe default, so an old record reads as "not known", never as
//! more, and writing back keeps what this version could not read. What still cannot be read fails
//! only itself: pages skip and report it.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fs_store::{ObjectId, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{CONTENT_TYPE, IDX_TEXT, SENSITIVE};
use super::message_index::add_index;
use super::preserve::keep_unknown;
use super::text::body_media_type;
use crate::error::Result;
use crate::model::{
    Attachment, DeliveryState, Encryption, ExternalId, Kind, Message, MessageSummary, Reaction,
    SenderKind, SignatureCheck, StoredForm, ToolCall,
};

// The stored form of independent facts about a message, not a state machine in disguise.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Content {
    #[serde(rename = "freesynergy.net:encryption")]
    encryption: Encryption,
    kind: Kind,
    #[serde(rename = "freesynergy.net:delivery")]
    state: DeliveryState,
    unread: bool,
    flagged: bool,
    preview: String,
    sender_kind: SenderKind,
    job_ref: String,
    #[serde(rename = "content")]
    body_text: String,
    #[serde(rename = "mediaType")]
    body_media_type: String,
    attachments: Vec<Attachment>,
    reactions: Vec<Reaction>,
    tool_calls: Vec<ToolCall>,
    tool_call_refs: BTreeMap<String, String>,
    task_refs: Vec<String>,
    reasoning: String,
    aliases: Vec<ExternalId>,
    links: Vec<ExternalId>,
    receipts: Vec<ExternalId>,
    alias_scope: String,
    account: Option<ObjectId>,
    outgoing: bool,
    pending_reply: Option<ExternalId>,
    references: Vec<ExternalId>,
    undecrypted: bool,
    signature: SignatureCheck,
    arrival: u64,
    sort_at: Option<DateTime<Utc>>,
    too_large: Option<u64>,
    body_blob: Option<ObjectId>,
}

/// Read a stored `Note`.
pub(super) fn message_from(o: StoredObject) -> Result<Message> {
    let raw = o.content;
    let c: Content = serde_json::from_value(raw.clone())?;
    let indexed_text = o.index.get(IDX_TEXT).and_then(|v| v.first()).cloned();
    let original_media_type = o
        .original
        .as_ref()
        .map(|b| b.media_type.clone())
        .unwrap_or_default();
    Ok(Message {
        summary: MessageSummary {
            envelope: o.envelope,
            kind: c.kind,
            encryption: c.encryption,
            state: c.state,
            unread: c.unread,
            flagged: c.flagged,
            preview: c.preview,
            sender_kind: c.sender_kind,
            job_ref: c.job_ref,
            arrival: c.arrival,
            undecrypted: c.undecrypted,
            signature: c.signature,
        },
        body_text: c.body_text,
        body_html: String::new(),
        attachments: c.attachments,
        reactions: c.reactions,
        original: o.original,
        original_media_type,
        tool_calls: c.tool_calls,
        tool_call_refs: c.tool_call_refs,
        task_refs: c.task_refs,
        reasoning: c.reasoning,
        body_media_type: body_media_type(&c.body_media_type),
        aliases: c.aliases,
        links: c.links,
        receipts: c.receipts,
        alias_scope: c.alias_scope,
        account: c.account,
        outgoing: c.outgoing,
        pending_reply: c.pending_reply,
        references: c.references,
        sort_at: c.sort_at,
        too_large: c.too_large,
        stored: StoredForm {
            body_blob: c.body_blob,
            indexed_text,
            raw: Some(raw),
        },
    })
}

/// The stored form. Always marked sensitive (ADR 0029), so a plain store refuses it.
pub(super) fn message_to(m: &Message) -> Result<StoredObject> {
    let s = &m.summary;
    let c = Content {
        encryption: s.encryption,
        kind: s.kind,
        state: s.state,
        unread: s.unread,
        flagged: s.flagged,
        preview: s.preview.clone(),
        sender_kind: s.sender_kind,
        job_ref: s.job_ref.clone(),
        // A body kept elsewhere is not copied by every small change.
        body_text: if m.stored.body_blob.is_some() {
            String::new()
        } else {
            m.body_text.clone()
        },
        body_media_type: m.body_media_type.clone(),
        attachments: m.attachments.clone(),
        reactions: m.reactions.clone(),
        tool_calls: m.tool_calls.clone(),
        tool_call_refs: m.tool_call_refs.clone(),
        task_refs: m.task_refs.clone(),
        reasoning: m.reasoning.clone(),
        aliases: m.aliases.clone(),
        links: m.links.clone(),
        receipts: m.receipts.clone(),
        alias_scope: m.alias_scope.clone(),
        account: m.account,
        outgoing: m.outgoing,
        pending_reply: m.pending_reply.clone(),
        references: m.references.clone(),
        undecrypted: s.undecrypted,
        signature: s.signature,
        arrival: s.arrival,
        sort_at: m.sort_at,
        too_large: m.too_large,
        body_blob: m.stored.body_blob,
    };
    let mut env = s.envelope.clone();
    env.sensitive = vec![SENSITIVE.to_string()];
    let mut content = serde_json::to_value(c)?;
    keep_unknown::<Content>(&mut content, m.stored.raw.as_ref())?;
    let mut o = StoredObject::new(env, CONTENT_TYPE, content);
    o.original.clone_from(&m.original);
    add_index(&mut o, m);
    Ok(o)
}
