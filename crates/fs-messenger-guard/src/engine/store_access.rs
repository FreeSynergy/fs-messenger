//! How the engine reaches its store: within a time limit, and fail-shut.
//!
//! Pattern: Decorator over the repository. Every read goes through, so a record gone bad is seen at
//! once; no answer in time reads as unreadable, and unreadable reads as stopped. A stop that could
//! not be written is laid over every read, and a task of its own keeps trying to write it until it
//! has: nobody has to call anything for that.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::MutexGuard;
use tokio::time::timeout;

use super::core::GuardEngine;
use crate::digest::unhex;
use crate::error::Refusal;
use crate::model::GuardMode;
use crate::state::{GuardDoc, StopMark};

/// What a read of the state found.
#[derive(Debug)]
pub(crate) enum View {
    /// There is none: a fresh program.
    Missing,
    /// The store did not answer.
    Unreadable(String),
    /// It answered with something that is not a state.
    Corrupt(String),
    /// A state.
    Doc(Box<GuardDoc>),
}

impl GuardEngine {
    /// What a device has to sign for: the engine's identity. `None` while there is no readable state.
    pub async fn engine_identity(&self) -> Option<Vec<u8>> {
        match self.view().await {
            View::Doc(doc) => unhex(&doc.engine),
            _ => None,
        }
    }

    /// What is wrong with the state, if it cannot be read. Such a state reads as stopped.
    pub async fn damage(&self) -> Option<String> {
        match self.view().await {
            View::Unreadable(why) | View::Corrupt(why) => Some(why),
            View::Missing | View::Doc(_) => None,
        }
    }

    pub(super) fn unsaved_mark(&self) -> Option<StopMark> {
        self.unsaved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The latch and the mark. From here on `check` refuses, and something keeps trying to write it.
    pub(super) fn mark_unsaved(&self, mark: StopMark) {
        self.stopped.store(true, Ordering::SeqCst);
        *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner) = Some(mark);
        self.ensure_retry();
    }

    pub(crate) fn clear_stop(&self) {
        *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.stopped.store(false, Ordering::SeqCst);
    }

    /// The lock, within the time limit, so that a stalled store cannot queue everyone up behind it.
    pub(crate) async fn lock_ops(&self) -> Result<MutexGuard<'_, ()>, Refusal> {
        timeout(self.config.store_wait * 2, self.ops.lock())
            .await
            .map_err(|_| Refusal::Unavailable)
    }

    pub(crate) async fn view(&self) -> View {
        self.view_within(self.config.store_wait).await
    }

    /// The state as it stands, with a stop that could not be written laid over it. Needs no lock.
    pub(super) async fn view_within(&self, wait: Duration) -> View {
        let Ok(loaded) = timeout(wait, self.repo.load()).await else {
            return View::Unreadable("the store did not answer in time".into());
        };
        match loaded {
            Err(why) => View::Unreadable(why),
            Ok(None) => View::Missing,
            Ok(Some(value)) => {
                let read = GuardDoc::from_value(value, &self.config.owner, self.verifier.as_ref());
                match read {
                    Err(why) => View::Corrupt(why),
                    Ok(mut doc) => {
                        if let Some(mark) = self.unsaved_mark() {
                            doc.apply_stop(&mark);
                        }
                        View::Doc(Box::new(doc))
                    }
                }
            }
        }
    }

    /// Write, within the time limit. A stop laid over this state is written with it, and is then no
    /// longer unsaved; that needs a state that really holds it.
    pub(crate) async fn save(&self, doc: &GuardDoc) -> Result<(), String> {
        let value = doc.to_value()?;
        timeout(self.config.store_wait, self.repo.save(&value))
            .await
            .map_err(|_| "the store did not answer in time".to_string())??;
        let mut unsaved = self.unsaved.lock().unwrap_or_else(PoisonError::into_inner);
        let holds = unsaved.as_ref().is_some_and(|mark| {
            doc.mode == GuardMode::Stopped
                && mark
                    .used
                    .as_ref()
                    .is_none_or(|u| doc.used_proofs.contains(u))
        });
        if holds {
            *unsaved = None;
        }
        Ok(())
    }

    /// One try. A failure stays as it was: reported when the stop was pulled, tried again next time.
    async fn try_write_stop(&self) {
        let Ok(_ops) = self.lock_ops().await else {
            return;
        };
        if let View::Doc(doc) = self.view().await {
            let _ = self.save(&doc).await;
        }
    }

    /// While a stop is not written, a task keeps trying. It holds the engine only for each try, and
    /// ends when the stop is written or the engine has gone. Outside a runtime it starts with the next
    /// `check`, which calls this too.
    pub(super) fn ensure_retry(&self) {
        if self.unsaved_mark().is_none() {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.retrying.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.me.clone();
        let every = self.config.stop_retry;
        handle.spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(engine) = me.upgrade() else {
                    return;
                };
                engine.try_write_stop().await;
                if engine.unsaved_mark().is_none() {
                    engine.retrying.store(false, Ordering::SeqCst);
                    // A stop that came in between has seen `retrying` set.
                    engine.ensure_retry();
                    return;
                }
            }
        });
    }
}
