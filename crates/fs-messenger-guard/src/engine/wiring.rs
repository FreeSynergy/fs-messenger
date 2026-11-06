//! How the engine gets into the messenger.
//!
//! Pattern: Adapter: the core's hooks over the engine. The core holds `Route`, and the engine holds a
//! port with a weak way back, so there is no cycle. `sender_kind` takes no lock of the engine: the core
//! calls it holding its write lock, and the engine holds its own while it writes into the messenger.
//! Over the guard's own invited list because `fs-people` pins `fs-store` v0.2.0 and this repo v0.3.0,
//! so that crate cannot be a dependency here; it is what the card lookup replaces, provisional.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fs_messenger_core::{
    EngineGuard, Error, MessengerRepo, Result as CoreResult, SenderKind, SenderLookup,
    ToolCallDecision,
};
use fs_store::ObjectId;

use super::core::GuardEngine;
use super::store_access::View;
use crate::error::{GuardError, Refusal};
use crate::identity::id_key;
use crate::model::GuardMode;

struct Route(Arc<GuardEngine>);

#[async_trait]
impl EngineGuard for Route {
    async fn decide(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        decision: ToolCallDecision,
    ) -> CoreResult<()> {
        match self.0.decide_call(conversation, message, &decision).await {
            Err(GuardError::Refused(refusal)) => Err(Error::DecisionRefused {
                problem: refusal.decision_problem(),
                detail: refusal.to_string(),
            }),
            other => Ok(other?),
        }
    }

    async fn stop_conversation(&self, conversation: ObjectId) -> CoreResult<()> {
        Ok(self.0.stop_conversation_now(conversation).await?)
    }

    async fn admit_agent_message(&self, sender: &str) -> CoreResult<()> {
        let wait = self.0.config.sender_wait;
        self.0
            .admission(sender, wait)
            .await
            .map_err(GuardError::from)?;
        Ok(())
    }
}

#[async_trait]
impl SenderLookup for Route {
    async fn sender_kind(&self, sender: &str) -> SenderKind {
        match self.0.view_within(self.0.config.sender_wait).await {
            View::Doc(doc) if doc.is_invited(&id_key(sender)) => SenderKind::Ai,
            _ => SenderKind::Unknown,
        }
    }
}

impl GuardEngine {
    /// May this participant's message come in? No lock: the way in must not wait for the guard's own
    /// writes. What cannot be said now is `Unavailable`: a sync stops there and loses nothing.
    pub(super) async fn admission(&self, sender: &str, wait: Duration) -> Result<(), Refusal> {
        if self.is_latched() {
            return Err(Refusal::Stopped);
        }
        match self.view_within(wait).await {
            View::Doc(doc) if doc.mode != GuardMode::Open => Err(Refusal::Stopped),
            View::Doc(doc) if doc.is_invited(&id_key(sender)) => Ok(()),
            View::Doc(_) => Err(Refusal::NotInvited),
            View::Missing => Err(Refusal::Stopped),
            View::Unreadable(_) | View::Corrupt(_) => Err(Refusal::Unavailable),
        }
    }

    /// Put this guard into a messenger: `decide` and `stop` go here from now on. A failure is fatal for
    /// the program: without it those answer `Unsupported`, and no question can be asked. Works once
    /// per guard and messenger, before any provider is registered, and not on the messenger's own store:
    /// the hub pushes a store's log. Without it the stop still works, and says what it could not reach.
    pub fn attach(self: &Arc<Self>, messenger: &MessengerRepo) -> Result<(), GuardError> {
        if self.port.get().is_some() {
            return Err(GuardError::AttachRefused("attached already".into()));
        }
        if self.repo.store_address() == Some(messenger.store_address()) {
            return Err(GuardError::AttachRefused(
                "the guard needs a store of its own".into(),
            ));
        }
        let route = Arc::new(Route(self.clone()));
        let port = messenger
            .install_guard(route.clone(), route)
            .map_err(|e| GuardError::AttachRefused(e.to_string()))?;
        self.port
            .set(port)
            .map_err(|_| GuardError::AttachRefused("attached already".into()))
    }
}
