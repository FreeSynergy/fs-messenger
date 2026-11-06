//! The end of a turn: store what there is, show the tool calls, count the usage, say how it ended.
//!
//! Pattern: Template method, in this order, so the state event is the last thing a listener hears. The
//! answer goes in as an incoming message of an `agent` account: the core asks the guard, and takes
//! `sender_kind` from the guard's lookup. This code says neither. A message that failed has to say so in
//! its name, because the core gives a provider no way to store `failed` (`DESIGN.md`, "Wanted").

use fs_messenger_core::{ExternalId, Ingested, Kind, RawMessage, TextPart, ToolCall, Usage};
use fs_store::ObjectId;

use super::failure::TurnFailure;
use super::run::{Said, TurnInput};
use super::shared::Shared;
use crate::provider::Host;
use crate::wire::TurnUsage;

const TURN_SCHEME: &str = "model-turn";
const MEDIA_TYPE: &str = "text/markdown";
const REASONING_CUT: &str = "\n[reasoning cut off here]";

pub(super) async fn finish(
    shared: &Shared,
    input: &TurnInput,
    live: ObjectId,
    mut said: Said,
    outcome: Result<(), TurnFailure>,
    job_ref: &str,
) {
    if let Some((at, piece)) = said.body.flush() {
        shared.live.text(live, TextPart::Body, at, piece);
    }
    if let Some((at, piece)) = said.reasoning.flush() {
        shared.live.text(live, TextPart::Reasoning, at, piece);
    }
    let held = shared.live.turn(live);
    let (body, mut reasoning) = held.map_or_else(Default::default, |t| (t.body, t.reasoning));
    if said.reasoning.was_cut() {
        reasoning.push_str(REASONING_CUT);
    }
    let failure = outcome.err();
    // A model the guard refused gets nothing stored; that is what the guard's word means.
    let stored = if failure == Some(TurnFailure::NotAdmitted) {
        None
    } else {
        store(shared, input, &body, reasoning, failure.as_ref(), job_ref).await
    };
    if let Some((host, id, _)) = &stored {
        shared.live.stored(live, *id);
        record_calls(host.as_ref(), *id, said.tools.finished(&shared.redactor)).await;
        record_usage(host.as_ref(), input.conversation, said.usage).await;
    }
    let failure = failure.or(match &stored {
        None => Some(TurnFailure::NotStored),
        Some((_, _, true)) => Some(TurnFailure::ResponseTooLarge),
        Some(_) => None,
    });
    if failure.is_none() {
        shared
            .history
            .finished(&input.conversation_alias.id, input.text.clone(), body);
    }
    shared.live.end(live, failure);
}

async fn store(
    shared: &Shared,
    input: &TurnInput,
    body: &str,
    reasoning: String,
    failure: Option<&TurnFailure>,
    job_ref: &str,
) -> Option<(std::sync::Arc<dyn Host>, ObjectId, bool)> {
    let host = shared.host().ok()?;
    let mut raw = RawMessage::new(
        ExternalId::new(TURN_SCHEME, input.turn_id.clone()),
        input.conversation_alias.clone(),
        Kind::Chat,
        shared.config.profile.clone(),
        body,
    );
    raw.body_media_type = MEDIA_TYPE.to_string();
    raw.reasoning = reasoning;
    raw.encryption = shared.encryption();
    raw.in_reply_to = Some(input.parent_alias.clone());
    raw.job_ref = job_ref.to_string();
    raw.subject = failure.map(|f| format!("Answer cut off: {f}"));
    // Over the core's limit only a stub stays. The limits here keep an answer below it, unless a program
    // set them higher; then the person reads `failed` and the live text, not a success.
    match host.ingest(shared.account, raw).await {
        Ok(Ingested::Stored(id) | Ingested::Duplicate(id)) => Some((host, id, false)),
        Ok(Ingested::TooLarge { id, .. }) => Some((host, id, true)),
        Err(_) => None,
    }
}

/// Each goes through the core's one way in. A call the core refuses stays unshown; the message is there.
async fn record_calls(host: &dyn Host, message: ObjectId, calls: Vec<(String, ToolCall)>) {
    for (reference, call) in calls {
        let _ = host.record_tool_call(message, &reference, call).await;
    }
}

/// `record_usage` replaces the total, so the new total starts from what is held. Cost stays as it is:
/// neither API states one, and this provider does not compute one from a price list.
async fn record_usage(host: &dyn Host, conversation: ObjectId, usage: Option<TurnUsage>) {
    let Some(usage) = usage else {
        return;
    };
    let mut total: Usage = host.usage(conversation).await.unwrap_or_default();
    total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
    if usage.context_used > 0 {
        total.context_used = usage.context_used;
    }
    let _ = host.record_usage(conversation, total).await;
}
