//! Tool calls as stored data: store, update in place, read (ADR 0054).
//!
//! Pattern: Adapter at the engine's boundary: what an agent reports is mapped onto `ToolCall` so
//! that it can claim nothing. Calls reach this version only from outside the engine, so they read
//! as `unknown` without options. Nobody here can answer, so an outside question is `working`: it is
//! shown and not asked. The guard follows, see `DESIGN.md`.
//!
//! Also Strategy, through the trait `EngineGuard::open_for_reports`: a report from a sender who is not
//! known to be a person or plain software is kept as `cancelled` while the guard says it is not open
//! (stopped, or cannot say). The core knows the trait, not the guard; a stop settles what is stored,
//! and this keeps a late report from showing `working` again.

use fs_store::ObjectId;

use super::guard_port::GUARD_WAIT;
use super::handle::Inner;
use super::message_codec::message_to;
use crate::error::{Error, Result};
use crate::model::{Message, SenderKind, ToolCall, ToolCallOutcome, ToolCallRisk, ToolCallState};

/// Take the agent's fields only. `id` and `revision` are assigned by the caller.
fn from_agent(mut call: ToolCall) -> ToolCall {
    call.risk = ToolCallRisk::Unknown;
    call.options.clear();
    call.arguments_digest.clear();
    call.asked_of.clear();
    if call.state == ToolCallState::Question {
        call.state = ToolCallState::Working;
    }
    if !call.detail.is_empty() {
        let detail = std::mem::take(&mut call.detail);
        call.summary = if call.summary.is_empty() {
            detail
        } else {
            format!("{}\n{detail}", call.summary)
        };
    }
    call
}

/// Apply one report to a message, and answer the call as it now stands, plus whether it changed.
pub(super) fn apply(message: &mut Message, agent_ref: &str, call: ToolCall) -> (ToolCall, bool) {
    let mut clean = from_agent(call);
    let held = message
        .tool_call_refs
        .get(agent_ref)
        .and_then(|id| message.tool_calls.iter().position(|c| &c.id == id));
    let Some(at) = held else {
        clean.id = ObjectId::new().to_string();
        clean.revision = 1;
        message
            .tool_call_refs
            .insert(agent_ref.to_string(), clean.id.clone());
        message.tool_calls.push(clean.clone());
        return (clean, true);
    };
    let current = &mut message.tool_calls[at];
    if current.state == ToolCallState::Settled {
        return (current.clone(), false);
    }
    clean.id.clone_from(&current.id);
    clean.revision = current.revision;
    if clean == *current {
        return (clean, false);
    }
    clean.revision += 1;
    *current = clean.clone();
    (clean, true)
}

const BESIDE_QUESTION: &str = "this message holds a question of the engine; no report lands here";

/// Only the engine ever sets `asked_of`: a report has it taken away.
fn holds_engine_call(message: &Message) -> bool {
    message.tool_calls.iter().any(|c| !c.asked_of.is_empty())
}

/// Settled as `cancelled`: what a report becomes while the guard is shut.
fn cancelled(mut call: ToolCall) -> ToolCall {
    call.state = ToolCallState::Settled;
    call.outcome = ToolCallOutcome::Cancelled;
    call
}

impl Inner {
    /// Only a person or plain software reports freely. `Ai` and `Unknown` need the guard to be open;
    /// without a guard nothing can be stopped, so the report stays as it is.
    async fn report_is_shut(&self, message: &Message) -> bool {
        if matches!(
            message.summary.sender_kind,
            SenderKind::Person | SenderKind::Software
        ) {
            return false;
        }
        let Some(wiring) = self.wiring.get() else {
            return false;
        };
        let sender = message.summary.envelope.attributed_to.as_deref();
        let open = tokio::time::timeout(
            GUARD_WAIT,
            wiring.guard.open_for_reports(sender.unwrap_or_default()),
        )
        .await;
        !open.unwrap_or(false)
    }

    /// A report may not land beside a question of the engine. It gets no right either way.
    async fn report_may_land(&self, id: ObjectId) -> Result<()> {
        let message = self.load_note(id).await?;
        if holds_engine_call(&message) {
            return Err(Error::GuardRefused(BESIDE_QUESTION.into()));
        }
        Ok(())
    }

    pub(super) async fn record_tool_call(
        &self,
        id: ObjectId,
        agent_ref: &str,
        call: ToolCall,
    ) -> Result<ToolCall> {
        if agent_ref.is_empty() {
            return Err(Error::Invalid(
                "a tool call needs the agent's reference, or a replay adds it again".into(),
            ));
        }
        self.report_may_land(id).await?;
        let _writes = self.writes.lock().await;
        let mut message = self.load_note(id).await?;
        // Checked again under the lock: nothing lands beside a person's question.
        if holds_engine_call(&message) {
            return Err(Error::GuardRefused(BESIDE_QUESTION.into()));
        }
        // Under the write lock; asked again after the write (below), because a stop's settling does
        // not wait for this lock before it reads the index.
        let call = if self.report_is_shut(&message).await {
            cancelled(call)
        } else {
            call
        };
        let (mut stored, changed) = apply(&mut message, agent_ref, call);
        let size = message.body_text.len() + serde_json::to_string(&message.tool_calls)?.len();
        let limit = self.config.max_message_bytes;
        if changed && size > limit {
            return Err(Error::TooLarge { size, limit });
        }
        if changed {
            self.put(message_to(&message)?).await?;
            // A stop may have come in between the question above and the write: its settling asks
            // the index before it takes the write lock, so it may not have seen this message. Asked
            // again after the write: a stop's latch is up before its settling begins, so either the
            // settling saw the message, or this look sees the latch.
            if stored.state != ToolCallState::Settled && self.report_is_shut(&message).await {
                let (shut, _) = apply(&mut message, agent_ref, cancelled(stored));
                self.put(message_to(&message)?).await?;
                stored = shut;
            }
        }
        Ok(stored)
    }
}
