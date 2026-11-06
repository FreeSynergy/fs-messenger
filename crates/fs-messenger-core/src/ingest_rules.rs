//! What a delivered message must be before the flow takes it.
//!
//! Pattern: Specification: pure rules, so they hold for every provider. A named refusal counts as
//! rejected in a sync and does not stop it.

use crate::error::{Error, Result};
use crate::model::ExternalId;
use crate::provider::RawMessage;

/// A chain longer than this keeps its newest entries; a sender decides how long it is.
const MAX_REFERENCES: usize = 64;

fn bad_part(alias: &ExternalId) -> bool {
    let bad = |s: &str| s.is_empty() || s.contains(['|', '\0']);
    bad(&alias.scheme) || bad(&alias.id)
}

fn size_of(raw: &RawMessage) -> usize {
    let files: usize = raw.attachments.iter().map(|a| a.data.len()).sum();
    let original = raw.original.as_ref().map_or(0, |o| o.bytes.len());
    raw.body_text.len() + raw.reasoning.len() + files + original
}

fn scrub(text: &mut String) {
    if text.contains('\0') {
        *text = text.replace('\0', "");
    }
}

/// The message as the flow may take it, or the refusal.
pub(crate) fn accepted(mut raw: RawMessage, limit: usize) -> Result<RawMessage> {
    let size = size_of(&raw);
    if size > limit {
        return Err(Error::TooLarge { size, limit });
    }
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
    if raw.aliases.iter().chain([&raw.conversation]).any(bad_part) {
        return Err(Error::Invalid(
            "an alias needs a scheme and an id, without `|` or NUL".into(),
        ));
    }
    if raw.tool_calls.iter().any(|t| t.agent_ref.is_empty()) {
        return Err(Error::Invalid(
            "a tool call needs the agent's reference".into(),
        ));
    }
    // The sender writes these. A bad one loses the claim, never the mail.
    raw.links.retain(|l| !bad_part(l));
    raw.references.retain(|r| !bad_part(r));
    let extra = raw.references.len().saturating_sub(MAX_REFERENCES);
    raw.references.drain(..extra);
    raw.in_reply_to = raw.in_reply_to.filter(|p| !bad_part(p));
    // The original keeps every byte; these are what is indexed and shown.
    scrub(&mut raw.body_text);
    scrub(&mut raw.reasoning);
    scrub(&mut raw.sender);
    scrub(&mut raw.job_ref);
    raw.subject.iter_mut().for_each(scrub);
    raw.conversation_name.iter_mut().for_each(scrub);
    raw.to
        .iter_mut()
        .chain(&mut raw.cc)
        .chain(&mut raw.task_refs)
        .for_each(scrub);
    Ok(raw)
}
