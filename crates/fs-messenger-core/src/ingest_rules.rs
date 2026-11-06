//! What a delivered message must be before the flow takes it.
//!
//! Pattern: Specification: pure rules, so they hold for every provider. A named refusal counts as
//! rejected in a sync and does not stop it.

use crate::error::{Error, Result};
use crate::model::ExternalId;
use crate::provider::RawMessage;

fn bad_part(alias: &ExternalId) -> bool {
    let bad = |s: &str| s.is_empty() || s.contains(['|', '\0']);
    bad(&alias.scheme) || bad(&alias.id)
}

fn scrub(text: &mut String) {
    if text.contains('\0') {
        *text = text.replace('\0', "");
    }
}

/// The message as the flow may take it, or the refusal.
pub(crate) fn accepted(mut raw: RawMessage) -> Result<RawMessage> {
    if raw.aliases.is_empty() {
        return Err(Error::Invalid(
            "a provider must give every message at least one alias, creating its own if needed"
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
