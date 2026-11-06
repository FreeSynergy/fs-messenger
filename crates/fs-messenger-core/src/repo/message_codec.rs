//! Message <-> stored object: AS2 `Note` envelope + content + index fields + immutable original.
//!
//! Pattern: Adapter: maps the [`Message`] view onto what `fs-store` holds (ADR 0020, 0030). Every
//! field of the content has a fail-safe default, so an old record reads as "not known", never as more.

use std::collections::BTreeMap;

use fs_store::{ObjectId, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{
    CONTENT_TYPE, IDX_ACCOUNT, IDX_ALIAS, IDX_BLOB, IDX_DESC, IDX_ENCRYPTION, IDX_FLAGGED,
    IDX_FROM, IDX_HAS_ATTACHMENT, IDX_JOB, IDX_KIND, IDX_ORDER, IDX_REPLY_ALIAS, IDX_TASK,
    IDX_TEXT, IDX_UNREAD, SENSITIVE, YES,
};
use super::keys::{alias_key, desc_key, order_key};
use super::text::body_media_type;
use crate::error::Result;
use crate::model::{
    Attachment, DeliveryState, Encryption, ExternalId, Kind, Message, MessageSummary, Reaction,
    SenderKind, ToolCall,
};

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
    account: Option<ObjectId>,
    outgoing: bool,
    pending_reply: Option<ExternalId>,
}

/// Read a stored `Note`.
pub(super) fn message_from(o: StoredObject) -> Result<Message> {
    let c: Content = serde_json::from_value(o.content)?;
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
        account: c.account,
        outgoing: c.outgoing,
        pending_reply: c.pending_reply,
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
        body_text: m.body_text.clone(),
        body_media_type: m.body_media_type.clone(),
        attachments: m.attachments.clone(),
        reactions: m.reactions.clone(),
        tool_calls: m.tool_calls.clone(),
        tool_call_refs: m.tool_call_refs.clone(),
        task_refs: m.task_refs.clone(),
        reasoning: m.reasoning.clone(),
        aliases: m.aliases.clone(),
        account: m.account,
        outgoing: m.outgoing,
        pending_reply: m.pending_reply.clone(),
    };
    let mut env = s.envelope.clone();
    env.sensitive = vec![SENSITIVE.to_string()];
    let mut o = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(c)?);
    o.original.clone_from(&m.original);
    add_index(&mut o, m);
    Ok(o)
}

fn add_index(o: &mut StoredObject, m: &Message) {
    let s = &m.summary;
    let (id, at) = (s.envelope.id, s.envelope.published);
    let mut add = |key: &str, value: String| {
        if !value.is_empty() {
            o.index.entry(key.to_string()).or_default().push(value);
        }
    };
    add(IDX_ORDER, order_key(at, id));
    add(IDX_DESC, desc_key(at, id));
    add(IDX_ENCRYPTION, s.encryption.as_str().to_string());
    add(IDX_KIND, s.kind.as_str().to_string());
    add(
        IDX_FROM,
        s.envelope.attributed_to.clone().unwrap_or_default(),
    );
    add(IDX_TEXT, m.body_text.clone());
    add(IDX_JOB, s.job_ref.clone());
    let flag = |on: bool| if on { YES.to_string() } else { String::new() };
    add(IDX_UNREAD, flag(s.unread));
    add(IDX_FLAGGED, flag(s.flagged));
    add(IDX_HAS_ATTACHMENT, flag(!m.attachments.is_empty()));
    for a in &m.attachments {
        add(IDX_BLOB, a.blob.to_string());
    }
    for t in &m.task_refs {
        add(IDX_TASK, t.clone());
    }
    if let Some(account) = m.account {
        add(IDX_ACCOUNT, account.to_string());
        for a in &m.aliases {
            add(IDX_ALIAS, alias_key(account, a));
        }
        if let Some(parent) = &m.pending_reply {
            add(IDX_REPLY_ALIAS, alias_key(account, parent));
        }
    }
}
