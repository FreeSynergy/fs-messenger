//! The index fields of a message.
//!
//! Pattern: none – verstößt gegen die none-Regel, an Strategie gemeldet, DESIGN.md Open point 0 (two functions, one of which changes the object it is given, so neither a single pure function nor a Specification; no pattern fits): what can be searched and sorted, derived in one place. An
//! undecrypted message or a stub has no text here: no ciphertext in search.

use fs_objects::StoredObject;

use super::consts::{
    IDX_ACCOUNT, IDX_ALIAS, IDX_ARRIVAL, IDX_ARRIVAL_DESC, IDX_BLOB, IDX_DESC, IDX_ENCRYPTION,
    IDX_FLAGGED, IDX_FROM, IDX_HAS_ATTACHMENT, IDX_JOB, IDX_KIND, IDX_LINK, IDX_OPEN_CALL,
    IDX_RECEIPT, IDX_REPLY_ALIAS, IDX_TASK, IDX_TEXT, IDX_UNREAD, YES,
};
use super::keys::{alias_key, arrival_desc_key, arrival_key, desc_key};
use super::text::indexable;
use crate::model::{Message, ToolCallState};

fn searchable(m: &Message) -> String {
    if m.summary.undecrypted || m.summary.too_large > 0 {
        return String::new();
    }
    match (&m.stored.body_blob, &m.stored.indexed_text) {
        (Some(_), Some(kept)) => kept.clone(),
        _ => indexable(&m.body_text).to_string(),
    }
}

pub(super) fn add_index(o: &mut StoredObject, m: &Message) {
    let s = &m.summary;
    let (id, at) = (s.envelope.id, m.sort_time());
    let mut add = |key: &str, value: String| {
        if !value.is_empty() {
            o.index.entry(key.to_string()).or_default().push(value);
        }
    };
    add(IDX_ARRIVAL, arrival_key(s.arrival, id));
    add(IDX_ARRIVAL_DESC, arrival_desc_key(s.arrival, id));
    add(IDX_DESC, desc_key(at, s.arrival, id));
    add(IDX_ENCRYPTION, s.encryption.as_str().to_string());
    add(IDX_KIND, s.kind.as_str().to_string());
    add(
        IDX_FROM,
        s.envelope.attributed_to.clone().unwrap_or_default(),
    );
    add(IDX_TEXT, searchable(m));
    add(IDX_JOB, s.job_ref.clone());
    let flag = |on: bool| if on { YES.to_string() } else { String::new() };
    add(IDX_UNREAD, flag(s.unread));
    add(IDX_FLAGGED, flag(s.flagged));
    let open = |c: &crate::model::ToolCall| c.state != ToolCallState::Settled;
    add(IDX_OPEN_CALL, flag(m.tool_calls.iter().any(open)));
    add(IDX_HAS_ATTACHMENT, flag(!m.attachments.is_empty()));
    for a in &m.attachments {
        add(IDX_BLOB, a.blob.to_string());
    }
    for t in &m.task_refs {
        add(IDX_TASK, t.clone());
    }
    if let Some(account) = m.account {
        add(IDX_ACCOUNT, account.to_string());
    }
    if !m.alias_scope.is_empty() {
        let scope = m.alias_scope.as_str();
        for (key, ids) in [
            (IDX_ALIAS, &m.aliases),
            (IDX_LINK, &m.links),
            (IDX_RECEIPT, &m.receipts),
        ] {
            for a in ids {
                add(key, alias_key(scope, a));
            }
        }
        if let Some(parent) = &m.pending_reply {
            add(IDX_REPLY_ALIAS, alias_key(scope, parent));
        }
    }
}
