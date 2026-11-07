//! The one place where an agent's tool call is stored: a message that arrives with calls in it
//! (`ingest_store.rs`) and a report on a stored message (`tool_calls.rs`) both go through here.
//!
//! Pattern: Facade, doubtful (section 2: "one small type that hides a subsystem"; `land_reports` is a method on `Inner`, not a type of its own; DESIGN.md open point 0): one entry that fixes the order "ask the guard, apply, write, ask again", so that no
//! way of storing a call can leave out a step. Also Strategy, through `EngineGuard::open_for_reports`:
//! a report from a sender who is not known to be a person or plain software is kept as `cancelled`
//! while the guard says it is not open (stopped, or cannot say). The core knows the trait, not the
//! guard. A stop settles what is stored and reads the index before it takes the write lock, so it can
//! miss a message that is being written at that moment: therefore the guard is asked a second time
//! after the write, and the call is cancelled if it has shut meanwhile (a stop's latch is up before
//! its settling begins: either the settling saw the message, or this look sees the latch).

use std::time::Duration;

use super::guard_port::GUARD_WAIT;
use super::handle::Inner;
use super::message_codec::message_to;
use super::tool_calls::apply;
use crate::error::{Error, Result};
use crate::model::{Message, SenderKind, ToolCall, ToolCallOutcome, ToolCallState};

/// Tries to write the cancelling, and the pause between them.
const CANCEL_TRIES: usize = 3;
const CANCEL_PAUSE: Duration = Duration::from_millis(50);

/// What is being stored.
#[derive(Clone, Copy, Debug)]
pub(super) enum Landing {
    /// A message that is not stored yet: it is written whether or not it has calls.
    New,
    /// A stored message: written only if a call changed, and not beyond `limit` bytes.
    Existing {
        /// Body plus calls, in bytes.
        limit: usize,
    },
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

    /// Apply the reports (agent reference and call) to the message, write it, and answer the calls as
    /// stored, in the same order. The caller holds the write lock. A failure of the first write is
    /// an error and stores nothing; the cancelling after it is tried again, and if it keeps failing
    /// that is an error too, while the first write stands (see `DESIGN.md` of the guard).
    pub(super) async fn land_reports(
        &self,
        message: &mut Message,
        reports: Vec<(String, ToolCall)>,
        landing: Landing,
    ) -> Result<Vec<ToolCall>> {
        let shut = self.report_is_shut(message).await;
        let mut stored = Vec::with_capacity(reports.len());
        let mut refs = Vec::with_capacity(reports.len());
        let mut changed = false;
        for (agent_ref, call) in reports {
            let call = if shut { cancelled(call) } else { call };
            let (call, did) = apply(message, &agent_ref, call);
            changed |= did;
            stored.push(call);
            refs.push(agent_ref);
        }
        if let Landing::Existing { limit } = landing {
            let size = message.body_text.len() + serde_json::to_string(&message.tool_calls)?.len();
            if changed && size > limit {
                return Err(Error::TooLarge { size, limit });
            }
        }
        if !changed && matches!(landing, Landing::Existing { .. }) {
            return Ok(stored);
        }
        self.put(message_to(message)?).await?;
        if !stored.iter().any(|c| c.state != ToolCallState::Settled)
            || !self.report_is_shut(message).await
        {
            return Ok(stored);
        }
        for (call, agent_ref) in stored.iter_mut().zip(&refs) {
            if call.state != ToolCallState::Settled {
                *call = apply(message, agent_ref, cancelled(call.clone())).0;
            }
        }
        self.put_again(message).await?;
        Ok(stored)
    }

    /// The cancelling must not be lost to one failed write.
    async fn put_again(&self, message: &Message) -> Result<()> {
        let mut last = Ok(());
        for attempt in 0..CANCEL_TRIES {
            if attempt > 0 {
                tokio::time::sleep(CANCEL_PAUSE).await;
            }
            last = self.put(message_to(message)?).await;
            if last.is_ok() {
                break;
            }
        }
        last
    }
}
