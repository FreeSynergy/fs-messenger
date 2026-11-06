//! One turn, from the first byte to the last.
//!
//! Pattern: Template method: fixed order, and each step can only end it as `failed`, with the text kept.
//! Wait until the core has recorded the person's message, so that a stop can name this conversation; ask
//! the guard again; connect; read within the limits; close; store (`finish.rs`).

use std::sync::Arc;
use std::time::Duration;

use fs_messenger_core::{DeliveryState, ExternalId, TextPart};
use fs_store::ObjectId;
use tokio::time::Instant;

use super::assemble::{PartText, ToolCalls};
use super::failure::TurnFailure;
use super::finish::finish;
use super::live::LiveOpening;
use super::registry::TurnTicket;
use super::shared::Shared;
use crate::wire::{ChatLine, Control, Ending, ModelEvent, SseReader, TurnUsage};

const RECORD_WAIT: Duration = Duration::from_secs(2);
const RECORD_POLL: Duration = Duration::from_millis(10);

/// What one turn needs to know.
pub(crate) struct TurnInput {
    pub(crate) conversation: ObjectId,
    pub(crate) conversation_alias: ExternalId,
    pub(crate) person_message: ObjectId,
    pub(crate) parent_alias: ExternalId,
    pub(crate) turn_id: String,
    pub(crate) text: String,
    pub(crate) lines: Vec<ChatLine>,
}

/// What a stream has said so far.
pub(super) struct Said {
    pub(super) body: PartText,
    pub(super) reasoning: PartText,
    pub(super) tools: ToolCalls,
    pub(super) usage: Option<TurnUsage>,
}

/// In a task of its own: the send has answered by then, and the core records the rest.
pub(crate) fn start(shared: Arc<Shared>, input: TurnInput, ticket: TurnTicket) {
    tokio::spawn(run(shared, input, ticket));
}

async fn run(shared: Arc<Shared>, input: TurnInput, ticket: TurnTicket) {
    let job_ref = format!("turn:{}", input.turn_id);
    let live = shared.live.open(&LiveOpening {
        conversation: input.conversation,
        job_ref: &job_ref,
        profile: &shared.config.profile,
        encryption: shared.encryption(),
    });
    let limits = &shared.config.limits;
    let mut said = Said {
        body: PartText::new(usize::MAX),
        reasoning: PartText::new(limits.reasoning_bytes),
        tools: ToolCalls::new(limits),
        usage: None,
    };
    let mut control = Control::new(ticket.cancel(), limits);
    let outcome = converse(&shared, &input, live, &mut said, &mut control).await;
    // `converse` has dropped the response, and with it the connection.
    ticket.closed();
    finish(&shared, &input, live, said, outcome, &job_ref).await;
    drop(ticket);
}

async fn converse(
    shared: &Shared,
    input: &TurnInput,
    live: ObjectId,
    said: &mut Said,
    control: &mut Control,
) -> Result<(), TurnFailure> {
    recorded(shared, input, control).await?;
    // Between the send and now the guard may have stopped; a stop before this turn registered reaches
    // it here, because the guard sets its latch before it asks providers.
    shared
        .gate
        .admit_message(&shared.config.profile)
        .await
        .map_err(|_| TurnFailure::NotAdmitted)?;
    let body = shared.dialect.body(&shared.config, &input.lines);
    let body = serde_json::to_vec(&body).map_err(|_| TurnFailure::RequestTooLarge)?;
    let headers = shared
        .dialect
        .headers(shared.config.api_key.as_ref())
        .map_err(|_| TurnFailure::Connection)?;
    let url = shared.config.endpoint.url(shared.dialect.path());
    let mut response = shared.transport.open(&url, &headers, body, control).await?;

    let limits = &shared.config.limits;
    let mut reader = shared.dialect.reader();
    let mut sse = SseReader::new(limits.event_bytes);
    let mut taken = 0usize;
    loop {
        let chunk = control
            .bounded(response.chunk())
            .await?
            .map_err(|_| TurnFailure::BrokeOff)?;
        let Some(chunk) = chunk else {
            let events = reader.end_of_body()?;
            return apply_all(shared, live, said, events).map(|_| ());
        };
        taken = taken.saturating_add(chunk.len());
        if taken > limits.response_bytes {
            return Err(TurnFailure::ResponseTooLarge);
        }
        for event in sse.feed(&chunk)? {
            let events = reader.event(&event)?;
            if apply_all(shared, live, said, events)? {
                return Ok(());
            }
        }
    }
}

/// `true` if the stream has ended; what it said after that is not read.
fn apply_all(
    shared: &Shared,
    live: ObjectId,
    said: &mut Said,
    events: Vec<ModelEvent>,
) -> Result<bool, TurnFailure> {
    for event in events {
        match event {
            ModelEvent::Text(text) => {
                if let Some((at, piece)) = said.body.push(&text, &shared.redactor) {
                    shared.live.text(live, TextPart::Body, at, piece);
                }
            }
            ModelEvent::Reasoning(text) => {
                if let Some((at, piece)) = said.reasoning.push(&text, &shared.redactor) {
                    shared.live.text(live, TextPart::Reasoning, at, piece);
                }
            }
            ModelEvent::ToolStart {
                index,
                agent_ref,
                name,
            } => said.tools.start(index, &agent_ref, &name),
            ModelEvent::ToolArguments { index, part } => said.tools.arguments(index, &part),
            ModelEvent::ToolEnd { index } => said.tools.end(index),
            ModelEvent::Usage(usage) => said.usage = Some(usage),
            ModelEvent::End(Ending::Complete) => return Ok(true),
            ModelEvent::End(Ending::Cut(reason)) => return Err(TurnFailure::Cut(reason)),
        }
    }
    Ok(false)
}

/// Until the core has answered the send: then the conversation knows its alias, a stop can name it, and
/// an answer cannot make a second conversation. No word from the core means nothing goes out.
async fn recorded(
    shared: &Shared,
    input: &TurnInput,
    control: &mut Control,
) -> Result<(), TurnFailure> {
    let until = Instant::now() + RECORD_WAIT;
    let host = shared.host().map_err(|_| TurnFailure::NotRecorded)?;
    while Instant::now() < until {
        let message = control
            .bounded(host.message(input.conversation, input.person_message))
            .await?;
        if message.is_ok_and(|m| m.summary.state != DeliveryState::Sending) {
            return Ok(());
        }
        control.sleep(RECORD_POLL).await?;
    }
    Err(TurnFailure::NotRecorded)
}
