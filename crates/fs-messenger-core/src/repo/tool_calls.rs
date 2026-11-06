//! Tool calls as stored data: store, update in place, read (ADR 0054).
//!
//! Pattern: Adapter at the engine's boundary: what an agent reports is mapped onto `ToolCall` so
//! that it can claim nothing. Calls reach this version only from outside the engine, so they read
//! as `unknown` without options: shown, not answerable. The guard follows, see `DESIGN.md`.

use fs_store::ObjectId;

use super::handle::Inner;
use super::message_codec::message_to;
use crate::error::Result;
use crate::model::{Message, ToolCall, ToolCallRisk, ToolCallState};

/// Take the agent's fields only. `id` and `revision` are assigned by the caller.
fn from_agent(mut call: ToolCall) -> ToolCall {
    call.risk = ToolCallRisk::Unknown;
    call.options.clear();
    call.arguments_digest.clear();
    call.asked_of.clear();
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
    let offered_revision = call.revision;
    let mut clean = from_agent(call);
    let held = message
        .tool_call_refs
        .get(agent_ref)
        .filter(|_| !agent_ref.is_empty())
        .and_then(|id| message.tool_calls.iter().position(|c| &c.id == id));
    let Some(at) = held else {
        clean.id = ObjectId::new().to_string();
        clean.revision = 1;
        if !agent_ref.is_empty() {
            message
                .tool_call_refs
                .insert(agent_ref.to_string(), clean.id.clone());
        }
        message.tool_calls.push(clean.clone());
        return (clean, true);
    };
    let current = &mut message.tool_calls[at];
    let stale = offered_revision != 0 && offered_revision < current.revision;
    if current.state == ToolCallState::Settled || stale {
        return (current.clone(), false);
    }
    clean.id.clone_from(&current.id);
    clean.revision = current.revision + 1;
    let mut unchanged = clean.clone();
    unchanged.revision = current.revision;
    if unchanged == *current {
        return (current.clone(), false);
    }
    *current = clean.clone();
    (clean, true)
}

impl Inner {
    pub(super) async fn record_tool_call(
        &self,
        id: ObjectId,
        agent_ref: &str,
        call: ToolCall,
    ) -> Result<ToolCall> {
        let _writes = self.writes.lock().await;
        let mut message = self.load_note(id).await?;
        let (stored, changed) = apply(&mut message, agent_ref, call);
        if changed {
            self.put(message_to(&message)?).await?;
        }
        Ok(stored)
    }
}
