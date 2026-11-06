//! Which turns run, and how to end them.
//!
//! Pattern: Registry + RAII guard: a [`TurnTicket`] holds a conversation's one place, and giving it back
//! cannot be forgotten, also not by a panic. A stop only signals and waits for the connection to be
//! closed. It never waits for the store: the guard may hold its own lock while it asks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use fs_messenger_core::{ExternalId, TurnScope};
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};

use crate::error::AiError;

/// The guard gives all providers 5 seconds together.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// Scheme of what this provider calls a conversation.
pub(crate) const CONVERSATION_SCHEME: &str = "model-conversation";

struct Running {
    cancel: watch::Sender<bool>,
    closed: watch::Sender<bool>,
}

/// By the conversation's alias.
#[derive(Default)]
pub(crate) struct Registry {
    turns: Arc<Mutex<HashMap<String, Arc<Running>>>>,
}

/// One running turn's place.
pub(crate) struct TurnTicket {
    turns: Arc<Mutex<HashMap<String, Arc<Running>>>>,
    conversation: String,
    running: Arc<Running>,
}

impl TurnTicket {
    /// What tells this turn to end.
    pub(crate) fn cancel(&self) -> watch::Receiver<bool> {
        self.running.cancel.subscribe()
    }

    /// The connection is closed: no more bytes are read.
    pub(crate) fn closed(&self) {
        self.running.closed.send_replace(true);
    }
}

impl Drop for TurnTicket {
    fn drop(&mut self) {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.remove(&self.conversation);
    }
}

impl Registry {
    /// The conversation's place, unless a turn holds it.
    pub(crate) fn begin(&self, conversation: &str) -> Result<TurnTicket, AiError> {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if turns.contains_key(conversation) {
            return Err(AiError::Busy);
        }
        let running = Arc::new(Running {
            cancel: watch::channel(false).0,
            closed: watch::channel(false).0,
        });
        turns.insert(conversation.to_string(), running.clone());
        Ok(TurnTicket {
            turns: self.turns.clone(),
            conversation: conversation.to_string(),
            running,
        })
    }

    /// Whether a turn runs in this conversation.
    pub(crate) fn is_running(&self, conversation: &str) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.contains_key(conversation)
    }

    /// End what `scope` names, and answer when their connections are closed. Nothing running is fine.
    pub(crate) async fn stop(&self, scope: &TurnScope) -> Result<(), AiError> {
        let chosen: Vec<Arc<Running>> = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns
                .iter()
                .filter(|(alias, _)| names(scope, alias))
                .map(|(_, running)| running.clone())
                .collect()
        };
        for running in &chosen {
            running.cancel.send_replace(true);
        }
        let until = Instant::now() + CLOSE_WAIT;
        for running in &chosen {
            let mut closed = running.closed.subscribe();
            // A sender that has gone is a turn that has gone.
            let waited = timeout_at(until, closed.wait_for(|c| *c)).await;
            if waited.is_err() {
                return Err(AiError::Host(
                    "a turn did not close its connection in time".into(),
                ));
            }
        }
        Ok(())
    }
}

fn names(scope: &TurnScope, alias: &str) -> bool {
    match scope {
        TurnScope::All => true,
        TurnScope::Conversation(ids) => ids
            .iter()
            .any(|id: &ExternalId| id.scheme == CONVERSATION_SCHEME && id.id == alias),
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn one_turn_per_conversation_and_the_place_comes_back() {
        let registry = Registry::default();
        let ticket = registry.begin("c").unwrap();
        assert!(matches!(registry.begin("c"), Err(AiError::Busy)));
        assert!(registry.begin("d").is_ok(), "another conversation");
        drop(ticket);
        assert!(registry.begin("c").is_ok());
    }

    #[tokio::test]
    async fn a_turn_that_does_not_close_is_reported_not_called_ended() {
        let registry = Registry::default();
        let _ticket = registry.begin("c").unwrap();
        let answer = timeout(Duration::from_secs(4), registry.stop(&TurnScope::All));
        assert!(answer.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn another_conversations_alias_names_nothing() {
        let registry = Registry::default();
        let ticket = registry.begin("c").unwrap();
        let other = ExternalId::new(CONVERSATION_SCHEME, "d");
        let foreign = ExternalId::new("mail-thread", "c");
        registry
            .stop(&TurnScope::Conversation(vec![other, foreign]))
            .await
            .unwrap();
        assert!(!*ticket.cancel().borrow());
    }
}
