//! Asking providers to end their turns.
//!
//! Pattern: Template method over the `Provider` strategy: each provider runs in a task of its own, so
//! one that hangs or panics cannot hold up the others or the stop. Every answer is reported.

use std::sync::Arc;
use std::time::Duration;

use fs_store::ObjectId;
use tokio::time::{Instant, timeout_at};

use super::guard_port::GuardPort;
use crate::error::{Error, Result};
use crate::provider::{Provider, TurnScope};

/// How one provider answered `stop_turns`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnStopOutcome {
    /// It says its turns have ended.
    Ended,
    /// It cannot end a turn. Its work may go on.
    Unsupported(String),
    /// It tried and failed. Its work may go on.
    Failed(String),
    /// It did not answer in time, or crashed. Its work may go on.
    NoAnswer,
}

/// One provider's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnStop {
    /// The account the provider is registered for.
    pub account: ObjectId,
    /// What it answered.
    pub outcome: TurnStopOutcome,
}

fn outcome_of(answer: Result<()>) -> TurnStopOutcome {
    match answer {
        Ok(()) => TurnStopOutcome::Ended,
        Err(Error::Unsupported(u)) => TurnStopOutcome::Unsupported(u.reason().to_string()),
        Err(e) => TurnStopOutcome::Failed(e.to_string()),
    }
}

impl GuardPort {
    /// Ask providers to end their turns: all of them, or the one of `conversation`. Together they have
    /// `wait`. No lock is held meanwhile.
    pub async fn stop_turns(
        &self,
        conversation: Option<ObjectId>,
        wait: Duration,
    ) -> Result<Vec<TurnStop>> {
        let inner = self.inner()?;
        let (scope, providers): (_, Vec<(ObjectId, Arc<dyn Provider>)>) = match conversation {
            None => (TurnScope::All, inner.all_providers()),
            Some(id) => {
                let content = inner.load_conversation(id).await?.content;
                let provider = content
                    .account
                    .and_then(|account| Some((account, inner.provider(account)?)));
                (
                    TurnScope::Conversation(content.aliases),
                    provider.into_iter().collect(),
                )
            }
        };
        drop(inner);
        let tasks: Vec<_> = providers
            .into_iter()
            .map(|(account, provider)| {
                let scope = scope.clone();
                let task = tokio::spawn(async move { provider.stop_turns(&scope).await });
                (account, task)
            })
            .collect();
        let deadline = Instant::now() + wait;
        let mut answers = Vec::with_capacity(tasks.len());
        for (account, task) in tasks {
            let abort = task.abort_handle();
            let outcome = match timeout_at(deadline, task).await {
                Ok(Ok(answer)) => outcome_of(answer),
                Ok(Err(_)) => TurnStopOutcome::NoAnswer,
                Err(_) => {
                    abort.abort();
                    TurnStopOutcome::NoAnswer
                }
            };
            answers.push(TurnStop { account, outcome });
        }
        Ok(answers)
    }
}
