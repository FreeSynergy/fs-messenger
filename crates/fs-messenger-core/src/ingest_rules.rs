//! What a delivered message must be before the flow takes it, and how large it is.
//!
//! Pattern: Specification: pure rules, so they hold for every provider. A broken id is refused by
//! name; what a sender wrote wrongly loses the claim, never the mail. A message over the limit is
//! not refused here but judged: the flow keeps a stub of it (ADR 0030), so it is visible, not gone.

use crate::error::{Error, Result};
use crate::model::{ExternalId, SignatureCheck};
use crate::provider::RawMessage;

/// A chain longer than this keeps its newest entries; a sender decides how long it is.
const MAX_REFERENCES: usize = 64;
/// Ids a sender may link a message to; a sender decides how many there are.
const MAX_LINKS: usize = 64;
/// More files than this is treated as too large, whatever they weigh.
const MAX_ATTACHMENTS: usize = 256;
/// Who, what about, what it is known as: more than this is treated as too large. These fields stay
/// in the message, so every change logs them again and every list carries them.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Without control characters: what a header, a name or a recipient may hold.
pub(crate) fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn strip(text: &mut String) {
    if text.contains(char::is_control) {
        *text = printable(text);
    }
}

fn scrub(text: &mut String) {
    if text.contains('\0') {
        *text = text.replace('\0', "");
    }
}

fn ids(list: &[ExternalId]) -> usize {
    list.iter().map(|a| a.scheme.len() + a.id.len()).sum()
}

fn texts(list: &[String]) -> usize {
    list.iter().map(String::len).sum()
}

/// Who, what about, what it is known as.
fn header_size(raw: &RawMessage) -> usize {
    let one = |t: &Option<String>| t.as_ref().map_or(0, String::len);
    let files: usize = raw
        .attachments
        .iter()
        .map(|a| a.name.len() + a.media_type.len())
        .sum();
    raw.sender.len()
        + raw.job_ref.len()
        + raw.body_media_type.len()
        + one(&raw.subject)
        + one(&raw.conversation_name)
        + texts(&raw.to)
        + texts(&raw.cc)
        + texts(&raw.task_refs)
        + ids(&raw.aliases)
        + ids(&raw.links)
        + ids(&raw.references)
        + ids(std::slice::from_ref(&raw.conversation))
        + raw
            .in_reply_to
            .as_ref()
            .map_or(0, |p| ids(std::slice::from_ref(p)))
        + files
}

/// Everything else a message brings, none of which the original makes redundant. Tool calls count
/// here and not as a header: what a tool returned can rightly be larger.
fn metadata_size(raw: &RawMessage) -> usize {
    let calls: usize = raw
        .tool_calls
        .iter()
        .map(|t| t.agent_ref.len() + serde_json::to_string(&t.call).map_or(0, |s| s.len()))
        .sum();
    header_size(raw) + calls
}

/// The size the limit applies to: metadata plus the original, or plus what was decoded from it if
/// that is larger. The same bytes are not counted twice, and decoded parts cannot hide behind a
/// small original.
pub(crate) fn size_of(raw: &RawMessage) -> usize {
    let original = raw.original.as_ref().map_or(0, |o| o.bytes.len());
    let files: usize = raw.attachments.iter().map(|a| a.data.len()).sum();
    let decoded = raw.body_text.len() + raw.reasoning.len() + files;
    metadata_size(raw) + original.max(decoded)
}

/// The message as the flow may take it, and its size if it is over what is taken; or the refusal.
pub(crate) fn judged(mut raw: RawMessage, limit: usize) -> Result<(RawMessage, Option<usize>)> {
    let size = size_of(&raw);
    let over = (size > limit
        || raw.attachments.len() > MAX_ATTACHMENTS
        || header_size(&raw) > MAX_HEADER_BYTES)
        .then_some(size);
    // What a sender chose can only link, however a provider labels it.
    let (chosen, issued): (Vec<_>, Vec<_>) = std::mem::take(&mut raw.aliases)
        .into_iter()
        .partition(ExternalId::is_sender_chosen);
    raw.aliases = issued;
    raw.links.extend(chosen);
    if raw.aliases.is_empty() {
        return Err(Error::Invalid(
            "a provider must give every message an id it issued itself; a Message-ID is not one"
                .into(),
        ));
    }
    let well_formed = raw.aliases.iter().chain([&raw.conversation]);
    if !well_formed.into_iter().all(ExternalId::is_well_formed) {
        return Err(Error::Invalid(
            "an alias needs a scheme and an id of sane length, without `|` or control characters"
                .into(),
        ));
    }
    // The sender writes these. A bad one loses the claim, never the mail.
    raw.links.retain(ExternalId::is_well_formed);
    raw.links.truncate(MAX_LINKS);
    raw.references.retain(ExternalId::is_well_formed);
    let extra = raw.references.len().saturating_sub(MAX_REFERENCES);
    raw.references.drain(..extra);
    raw.in_reply_to = raw.in_reply_to.filter(ExternalId::is_well_formed);
    raw.tool_calls.retain(|t| !t.agent_ref.is_empty());
    // The original keeps every byte; these are what is indexed and shown.
    scrub(&mut raw.body_text);
    scrub(&mut raw.reasoning);
    // These can go on into a reply's recipients and a mail's headers: no CRLF, no other control.
    strip(&mut raw.sender);
    strip(&mut raw.job_ref);
    raw.subject.iter_mut().for_each(strip);
    raw.conversation_name.iter_mut().for_each(strip);
    raw.to
        .iter_mut()
        .chain(&mut raw.cc)
        .chain(&mut raw.task_refs)
        .for_each(strip);
    // A signature says who wrote what: without a sender or readable content it says nothing.
    if raw.undecrypted || raw.sender.trim().is_empty() {
        raw.signature = SignatureCheck::NotVerified;
    }
    Ok((raw, over))
}

/// What stays of a message over the limit: who, when, what it is known as. No content.
pub(crate) fn stub_of(raw: RawMessage) -> RawMessage {
    let short = |t: &str, n: usize| t.chars().take(n).collect::<String>();
    let mut stub = RawMessage::new(
        raw.aliases[0].clone(),
        raw.conversation,
        raw.kind,
        short(&raw.sender, 1024),
        "",
    );
    stub.aliases = raw.aliases.into_iter().take(8).collect();
    // Already capped; a reply to it still finds it.
    stub.links = raw.links;
    stub.conversation_name = raw.conversation_name.map(|n| short(&n, 200));
    stub.subject = Some(
        raw.subject
            .map_or_else(|| "(too large)".to_string(), |s| short(&s, 200)),
    );
    stub.encryption = raw.encryption;
    stub.published = raw.published;
    stub.outgoing = raw.outgoing;
    stub
}
