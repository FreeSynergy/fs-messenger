//! [`GuardPort`]: the only way to write what the engine itself says about a tool call.
//!
//! Pattern: Capability object + Facade: `install_guard` works once and hands out the one port, which
//! nothing else can build. `Messenger`, `Ingest` and `Provider` have no path to it. The port holds the
//! repo weakly: the repo holds the guard, and a cycle would keep the store's claim for ever.

use std::sync::{Arc, Weak};
use std::time::Duration;

use fs_store::{ObjectId, Query};
use tokio::time::timeout;

use super::accounts::is_agent;
use super::consts::{IDX_OPEN_CALL, NOTE, YES};
use super::guard_note::ENGINE_ACTOR;
use super::handle::{Inner, MessengerRepo};
use super::message_codec::{message_from, message_to};
use crate::error::{Error, Result};
use crate::guard_hook::{EngineGuard, SenderLookup};
use crate::model::{Event, SenderKind, ToolCall, ToolCallChange, ToolCallOutcome, ToolCallState};

/// How long the core waits for a guard, which it never does while holding its write lock.
const GUARD_WAIT: Duration = Duration::from_secs(1);

pub(super) struct Wiring {
    guard: Arc<dyn EngineGuard>,
    senders: Arc<dyn SenderLookup>,
}

/// Where a question was put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedCall {
    /// Its conversation.
    pub conversation: ObjectId,
    /// Its message.
    pub message: ObjectId,
}

/// What settling found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettledCalls {
    /// Calls now settled as `cancelled`.
    pub settled: usize,
    /// Marked records that could not be read, so what they show stays.
    pub unreadable: usize,
}

/// Held by the guard. Not `Clone` and without a public constructor.
pub struct GuardPort {
    repo: Weak<Inner>,
}

impl MessengerRepo {
    /// Install the guard. Works once, and only before any provider is registered: a provider holding
    /// a clone of the repo cannot get in first, and a guard cannot be replaced at run time.
    pub fn install_guard(
        &self,
        guard: Arc<dyn EngineGuard>,
        senders: Arc<dyn SenderLookup>,
    ) -> Result<GuardPort> {
        if !self.inner.all_providers().is_empty() {
            return Err(Error::Conflict(
                "install the guard before registering a provider".into(),
            ));
        }
        self.inner
            .wiring
            .set(Wiring { guard, senders })
            .map_err(|_| Error::Conflict("a guard is already installed".into()))?;
        Ok(GuardPort {
            repo: Arc::downgrade(&self.inner),
        })
    }
}

impl Inner {
    pub(super) fn guard(&self) -> Option<&Arc<dyn EngineGuard>> {
        self.wiring.get().map(|w| &w.guard)
    }

    /// What this sender is, and whether its message may be stored. The rule is in the guard's
    /// `DESIGN.md`: an account of kind `agent` carries AI participants only, so without a guard, or
    /// without the guard's word, nothing of it is stored. Called without any lock held, and a guard
    /// that does not answer cannot stall a messenger write: for a sender that reads as not known, and
    /// for an agent's message as "try again".
    pub(super) async fn admitted_sender(
        &self,
        account_kind: &str,
        sender: &str,
    ) -> Result<SenderKind> {
        let Some(wiring) = self.wiring.get() else {
            if is_agent(account_kind) {
                return Err(Error::GuardRefused(
                    "no guard is installed, so no AI participant exists".into(),
                ));
            }
            return Ok(SenderKind::Unknown);
        };
        if is_agent(account_kind) {
            match timeout(GUARD_WAIT, wiring.guard.admit_agent_message(sender)).await {
                Ok(answer) => answer?,
                Err(_) => {
                    return Err(Error::GuardUnavailable(
                        "the guard did not answer in time".into(),
                    ));
                }
            }
        }
        Ok(timeout(GUARD_WAIT, wiring.senders.sender_kind(sender))
            .await
            .unwrap_or(SenderKind::Unknown))
    }
}

impl GuardPort {
    pub(super) fn inner(&self) -> Result<Arc<Inner>> {
        self.repo
            .upgrade()
            .ok_or_else(|| Error::Conflict("the messenger has closed".into()))
    }

    /// Put an engine call, as given, into a new message: into `conversation`, or into the participant's
    /// own guard conversation.
    pub async fn place_question(
        &self,
        conversation: Option<ObjectId>,
        profile: &str,
        call: ToolCall,
    ) -> Result<PlacedCall> {
        let inner = self.inner()?;
        let _writes = inner.writes.lock().await;
        let record = match conversation {
            Some(id) => inner.load_conversation(id).await?,
            None => inner.guard_conversation(profile).await?,
        };
        let message = inner.engine_note(&record, profile, call).await?;
        let conversation = record.envelope.id;
        inner.refresh(conversation).await?;
        Ok(PlacedCall {
            conversation,
            message,
        })
    }

    /// Replace a call by its id, as given; an empty `detail` keeps the held one. `settled` stays final.
    pub async fn update_call(&self, message: ObjectId, call: ToolCall) -> Result<()> {
        let inner = self.inner()?;
        let _writes = inner.writes.lock().await;
        let mut m = inner.load_note(message).await?;
        let Some(held) = m.tool_calls.iter_mut().find(|c| c.id == call.id) else {
            return Err(Error::NotFound(message));
        };
        if held.state == ToolCallState::Settled {
            return Ok(());
        }
        // The engine keeps no arguments, so an update without a detail keeps the one shown.
        let detail = if call.detail.is_empty() {
            std::mem::take(&mut held.detail)
        } else {
            call.detail.clone()
        };
        *held = call;
        held.detail = detail;
        inner.store.put(message_to(&m)?, ENGINE_ACTOR).await?;
        Ok(())
    }

    /// Tell listeners how a call really stands, without writing anything: a refused decision changes
    /// nothing. An empty `detail` takes the one shown, if the message can still be read.
    pub async fn announce_call(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        mut call: ToolCall,
    ) -> Result<()> {
        let inner = self.inner()?;
        if call.detail.is_empty() {
            if let Ok(m) = inner.load_note(message).await {
                if let Some(held) = m.tool_calls.iter().find(|c| c.id == call.id) {
                    call.detail.clone_from(&held.detail);
                }
            }
        }
        // Nobody listening is no failure.
        let _ = inner.announcements.send(Event::ToolCall(ToolCallChange {
            message,
            call,
            conversation,
        }));
        Ok(())
    }

    /// Take away the message of a settled question of the engine. Nothing else can go this way: the
    /// message must hold nothing but settled engine calls.
    pub async fn delete_settled_question(&self, message: ObjectId) -> Result<()> {
        let inner = self.inner()?;
        let _writes = inner.writes.lock().await;
        let m = match inner.load_note(message).await {
            Ok(m) => m,
            Err(Error::NotFound(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        let engine_only = !m.tool_calls.is_empty()
            && m.tool_calls
                .iter()
                .all(|c| c.state == ToolCallState::Settled && !c.asked_of.is_empty());
        if !engine_only {
            return Ok(());
        }
        inner.store.delete(message, ENGINE_ACTOR).await?;
        if let Some(conversation) = m.summary.envelope.context.as_deref() {
            inner.refresh(conversation.parse()?).await?;
        }
        Ok(())
    }

    /// Settle what is open as `cancelled`, an agent's own reports too: all, or one conversation's.
    pub async fn settle_open_calls(&self, conversation: Option<ObjectId>) -> Result<SettledCalls> {
        let inner = self.inner()?;
        let mut query = Query::new().of_type(NOTE).with_index(IDX_OPEN_CALL, YES);
        if let Some(id) = conversation {
            query = query.in_context(id.to_string());
        }
        let mut found = SettledCalls::default();
        for object in inner.store.query(&query).await? {
            let id = object.envelope.id;
            let _writes = inner.writes.lock().await;
            let Some(object) = inner.store.get(id).await? else {
                continue;
            };
            let Ok(mut m) = message_from(object) else {
                found.unreadable += 1;
                continue;
            };
            for call in &mut m.tool_calls {
                if call.state != ToolCallState::Settled {
                    call.state = ToolCallState::Settled;
                    call.outcome = ToolCallOutcome::Cancelled;
                    call.options.clear();
                    call.revision += 1;
                    found.settled += 1;
                }
            }
            inner.store.put(message_to(&m)?, ENGINE_ACTOR).await?;
        }
        Ok(found)
    }
}
