//! [`Listener`] and [`Subscription`]: who hears about changes, and for how long.
//!
//! Pattern: Observer + RAII guard: dropping the [`Subscription`] ends it, as in the contract.

use async_trait::async_trait;
use tokio::task::AbortHandle;

use crate::model::Event;

/// Hears about changes, one event at a time, in log order.
#[async_trait]
pub trait Listener: Send + Sync {
    /// One change.
    async fn event(&self, event: Event);
}

/// A running subscription. Dropping it ends it.
#[derive(Debug)]
pub struct Subscription {
    task: AbortHandle,
}

impl Subscription {
    pub(crate) fn new(task: AbortHandle) -> Self {
        Self { task }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}
