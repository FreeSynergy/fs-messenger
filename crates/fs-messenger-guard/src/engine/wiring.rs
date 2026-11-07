//! How the engine gets into the messenger.
//!
//! Pattern: Adapter: the core's hooks over the engine. The core holds `Route`, and the engine holds a
//! port with a weak way back, so there is no cycle. `sender_kind` takes no lock of the engine: the core
//! calls it holding its write lock, and the engine holds its own while it writes into the messenger.
//! Over the guard's own invited list because `fs-people` pins `fs-objects` v0.2.0 and this repo v0.3.0,
//! so that crate cannot be a dependency here; it is what the card lookup replaces, provisional.

use std::sync::Arc;

use async_trait::async_trait;
use fs_messenger_core::{
    EngineGuard, Error, MessengerRepo, Result as CoreResult, SenderKind, SenderLookup,
    ToolCallDecision,
};
use fs_objects::ObjectId;

use super::core::{GuardEngine, View};
use crate::api::Gate;
use crate::error::{GuardError, Refusal};

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
        self.0
            .admit_message(sender)
            .await
            .map_err(GuardError::from)?;
        Ok(())
    }
}

#[async_trait]
impl SenderLookup for Route {
    async fn sender_kind(&self, sender: &str) -> SenderKind {
        match self.0.view().await {
            View::Doc(doc) if doc.is_invited(sender) => SenderKind::Ai,
            _ => SenderKind::Unknown,
        }
    }
}

impl GuardEngine {
    /// Put this guard into a messenger: `decide` and `stop` go here from now on. Works once per
    /// messenger and once per guard. Without it the stop still works; it has no message to settle and
    /// no provider to ask, and says so in its report.
    pub fn attach(self: &Arc<Self>, messenger: &MessengerRepo) -> Result<(), GuardError> {
        if self.port.get().is_some() {
            return Err(Refusal::NotEnrollable("this guard is attached already").into());
        }
        let route = Arc::new(Route(self.clone()));
        let port = messenger
            .install_guard(route.clone(), route)
            .map_err(|e| GuardError::NotPersisted(e.to_string()))?;
        self.port
            .set(port)
            .map_err(|_| Refusal::NotEnrollable("this guard is attached already").into())
    }
}
