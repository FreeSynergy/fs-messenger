//! The core's model as records of the type descriptions.
//!
//! Pattern: Adapter (pure functions, model in, `Record` out). Empty values are left out (the frontend shows "none"). A
//! value is always the canonical text of its basic type: a choice is a key of the description, a reference is an IRI.

use chrono::SecondsFormat;
use fs_admin_frontend::{Record, Values};
use fs_messenger_core::{
    Account, ConversationSummary, DeliveryState, DraftInfo, Message, MessageSummary, SenderKind,
    SignatureCheck,
};

use crate::shared::{iri, put, yes_no};

fn delivery(state: DeliveryState) -> &'static str {
    match state {
        DeliveryState::Sending => "sending",
        DeliveryState::Sent => "sent",
        DeliveryState::Delivered => "delivered",
        DeliveryState::Read => "read",
        DeliveryState::Failed => "failed",
        DeliveryState::Unknown => "unknown",
    }
}

fn sender(kind: SenderKind) -> &'static str {
    match kind {
        SenderKind::Person => "person",
        SenderKind::Software => "software",
        SenderKind::Ai => "ai",
        SenderKind::Unknown => "unknown",
    }
}

fn signature(check: SignatureCheck) -> &'static str {
    match check {
        SignatureCheck::NotVerified => "not-verified",
        SignatureCheck::NotSigned => "not-signed",
        SignatureCheck::Valid => "valid",
    }
}

/// A text that names an object by id is an IRI; anything else stays as it is.
fn as_reference(text: &str) -> String {
    text.parse::<fs_objects::ObjectId>()
        .map_or_else(|_| text.to_owned(), iri)
}

/// An account.
pub(crate) fn account(a: &Account) -> Record {
    let mut v = Values::new();
    put(&mut v, "address", a.address.clone());
    put(&mut v, "kind", a.kind.clone());
    put(&mut v, "encryptionReady", yes_no(a.encryption_ready));
    Record {
        id: iri(a.id),
        values: v,
    }
}

/// A conversation.
pub(crate) fn conversation(c: &ConversationSummary) -> Record {
    let mut v = Values::new();
    put(&mut v, "name", c.name.clone());
    put(&mut v, "kind", c.kind.as_str());
    put(&mut v, "encryption", c.encryption.as_str());
    put(&mut v, "unreadCount", c.unread_count.to_string());
    if let Some(last) = &c.last_message {
        put(&mut v, "lastMessage", iri(last.envelope.id));
    }
    put(&mut v, "pinned", yes_no(c.pinned));
    for f in &c.folders {
        put(&mut v, "folders", f.clone());
    }
    Record {
        id: iri(c.id),
        values: v,
    }
}

fn summary_values(s: &MessageSummary, v: &mut Values) {
    let e = &s.envelope;
    put(v, "name", e.name.clone());
    put(
        v,
        "attributedTo",
        e.attributed_to.clone().unwrap_or_default(),
    );
    for t in &e.to {
        put(v, "to", t.clone());
    }
    for c in &e.cc {
        put(v, "cc", c.clone());
    }
    put(
        v,
        "published",
        e.published.to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    put(
        v,
        "context",
        e.context.as_deref().map(as_reference).unwrap_or_default(),
    );
    put(
        v,
        "inReplyTo",
        e.in_reply_to
            .as_deref()
            .map(as_reference)
            .unwrap_or_default(),
    );
    put(v, "preview", s.preview.clone());
    put(v, "kind", s.kind.as_str());
    put(v, "encryption", s.encryption.as_str());
    put(v, "state", delivery(s.state));
    put(v, "unread", yes_no(s.unread));
    put(v, "flagged", yes_no(s.flagged));
    put(v, "senderKind", sender(s.sender_kind));
    put(v, "signature", signature(s.signature));
    put(v, "jobRef", s.job_ref.clone());
}

/// A message known only by its summary (the body could not be read, or the conversation is not known).
pub(crate) fn message_summary(s: &MessageSummary) -> Record {
    let mut v = Values::new();
    summary_values(s, &mut v);
    Record {
        id: iri(s.envelope.id),
        values: v,
    }
}

/// A whole message.
pub(crate) fn message(m: &Message) -> Record {
    let mut v = Values::new();
    summary_values(&m.summary, &mut v);
    if let Some(a) = m.account {
        put(&mut v, "account", iri(a));
    }
    put(&mut v, "content", m.body_text.clone());
    put(
        &mut v,
        "mediaType",
        if m.body_media_type == "text/markdown" {
            "markdown"
        } else {
            "plain"
        },
    );
    for a in &m.attachments {
        put(
            &mut v,
            "attachments",
            format!("{} ({}, {} bytes)", a.name, a.media_type, a.size),
        );
    }
    for r in &m.reactions {
        put(&mut v, "reactions", format!("{} x {}", r.key, r.count));
    }
    for t in &m.tool_calls {
        put(
            &mut v,
            "toolCalls",
            format!(
                "{} [{}]: {}",
                t.name,
                format!("{:?}", t.state).to_lowercase(),
                t.summary
            ),
        );
    }
    for t in &m.task_refs {
        put(&mut v, "taskRefs", t.clone());
    }
    Record {
        id: iri(m.summary.envelope.id),
        values: v,
    }
}

/// A draft.
pub(crate) fn draft(d: &DraftInfo) -> Record {
    let mut v = Values::new();
    for t in &d.to {
        put(&mut v, "to", t.clone());
    }
    put(&mut v, "text", d.text.clone());
    Record {
        id: iri(d.id),
        values: v,
    }
}
