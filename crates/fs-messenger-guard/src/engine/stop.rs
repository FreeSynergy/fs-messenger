//! The emergency stop.
//!
//! Pattern: Facade with a fixed order of steps, which is the guarantee. 1. Latch, before any read: no
//! store can hold this up. 2. Judge the proof, within a time limit (`judge_wait`; past it the stop is one by nobody); a proof shown to be bad takes that
//! latch back, and anything else leaves it. 3. Marker file, store, what is shown, the providers and
//! the log, each as a task of its own: they go on if the caller walks away, and one that hangs or
//! panics cannot hold up or take down another.
//!
//! A state that cannot be read holds no keys to check a proof against. It reads as stopped already, so
//! such a pull goes on without a proof, and can do nothing but end work.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::core::GuardEngine;
use super::marker;
use super::store_access::View;
use crate::error::{GuardError, Refusal};
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::model::{GuardProof, StopReport};
use crate::signed::Method;
use crate::state::{ProofRequest, StopMark};

const ENGINE: &str = "urn:freesynergy:messenger:guard";
/// Who a stop from the local console is by.
pub(crate) const LOCAL_CONSOLE: &str = "urn:freesynergy:messenger:local-console";

/// Wait for a task for so long. It goes on either way; the answer says what is known.
async fn await_task<T>(task: JoinHandle<T>, wait: Duration) -> Result<T, String> {
    match timeout(wait, task).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err("this step crashed".into()),
        Err(_) => Err("this step is still running".into()),
    }
}

impl GuardEngine {
    /// Who may pull it. Answers the mark, or why not. With no readable state there is no way to ask.
    async fn judge(&self, proof: Option<&GuardProof>) -> Result<StopMark, Refusal> {
        let at = self.clock.now();
        let Some(proof) = proof else {
            return Ok(StopMark {
                by: LOCAL_CONSOLE.to_string(),
                at,
                used: None,
            });
        };
        let View::Doc(doc) = self.view().await else {
            return Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            });
        };
        // Verifier and permission are plain calls that may block: a thread of their own, so that
        // the time limit of `judge_surely` can fire whatever they do.
        let (verifier, permission) = (self.verifier.clone(), self.permission.clone());
        let (owner, proof) = (self.config.owner.clone(), proof.clone());
        let judged = tokio::task::spawn_blocking(move || {
            let request = ProofRequest {
                method: Method::Stop,
                arguments: &[],
            };
            let used = doc.check_proof(verifier.as_ref(), &request, &proof)?;
            let signer = id_key(&proof.signer);
            if !permission.may_stop(&signer, &owner) {
                return Err(Refusal::NotPermitted);
            }
            Ok(StopMark {
                by: signer,
                at,
                used: Some(used),
            })
        });
        // A panic there is a stop by nobody, like no answer in time.
        judged.await.unwrap_or_else(|_| {
            Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            })
        })
    }

    /// `judge` in a task of its own, within `judge_wait`: a foreign verifier or permission that
    /// panics or never answers must not end this stop, nor hold it up, before its marker is written.
    /// What cannot be judged in time is a stop by nobody: fail-shut, as when the state cannot be read.
    async fn judge_surely(
        self: &Arc<Self>,
        proof: Option<GuardProof>,
    ) -> Result<StopMark, Refusal> {
        let judge = self.clone();
        let at = self.clock.now();
        let task = tokio::spawn(async move { judge.judge(proof.as_ref()).await });
        match timeout(self.config.judge_wait, task).await {
            Ok(Ok(judged)) => judged,
            // Panicked, or no answer in time.
            _ => Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            }),
        }
    }

    /// With a proof, or without one from the local console: the caller can walk away from this. The
    /// latch is up before this awaits anything.
    pub(crate) async fn pull_stop(
        &self,
        proof: Option<GuardProof>,
    ) -> Result<StopReport, GuardError> {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let engine = self.me.upgrade().ok_or(Refusal::Unavailable)?;
        let task = tokio::spawn(async move { engine.run_stop(proof).await });
        task.await
            .map_err(|_| GuardError::NotPersisted("the stop crashed; it stays stopped".into()))?
    }

    async fn run_stop(
        self: Arc<Self>,
        proof: Option<GuardProof>,
    ) -> Result<StopReport, GuardError> {
        let mark = match self.judge_surely(proof).await {
            Ok(mark) => mark,
            Err(refusal) => {
                self.pending.fetch_sub(1, Ordering::SeqCst);
                return Err(refusal.into());
            }
        };
        self.mark_unsaved(mark.clone());
        self.pending.fetch_sub(1, Ordering::SeqCst);

        let wait = self.config.store_wait * 4;
        let persist = tokio::spawn(self.clone().persist_stop(mark.clone()));
        let settle = tokio::spawn(self.clone().settle_all());
        let turns = tokio::spawn(self.clone().end_turns());
        let (not_marked, not_persisted) = await_task(persist, wait)
            .await
            .unwrap_or_else(|why| (None, Some(why)));
        let (calls_cancelled, not_shown) = await_task(settle, wait)
            .await
            .unwrap_or_else(|why| (0, Some(why)));
        let turn_wait = self.config.turn_wait + Duration::from_secs(1);
        let (turns, not_shown) = match await_task(turns, turn_wait).await {
            Ok(Ok(turns)) => (turns, not_shown),
            Ok(Err(why)) | Err(why) => (vec![], not_shown.or(Some(why))),
        };
        self.retries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();

        let mut entry = GuardLogEntry::new("stop", &mark.by);
        entry.cancelled = calls_cancelled;
        if entry.actor.is_empty() {
            entry.actor = ENGINE.to_string();
        }
        let logger = self.clone();
        let log = tokio::spawn(async move { logger.log_entry(&entry).await });
        let not_logged = match await_task(log, self.config.store_wait * 2).await {
            Ok(result) => result.err(),
            Err(why) => Some(why),
        };
        Ok(StopReport {
            not_persisted,
            not_marked,
            calls_cancelled,
            not_shown,
            turns,
            not_logged,
            messenger_attached: self.port.get().is_some(),
        })
    }

    /// The marker first: it does not depend on the store. Answers why the marker, and why the store,
    /// could not be written. Whatever fails here, the retry task goes on.
    async fn persist_stop(self: Arc<Self>, mark: StopMark) -> (Option<String>, Option<String>) {
        let wait = self.config.store_wait;
        let path = self.config.stop_marker.clone();
        let mark_it = marker::write(path.clone(), mark.clone());
        let mut not_marked = timeout(wait, mark_it)
            .await
            .unwrap_or_else(|_| Err("the disk did not answer in time".into()))
            .err();
        let Ok(_ops) = self.lock_ops().await else {
            // The mark stays (a `reopen` does not clear a stop that came in after it began), and the
            // retry task writes marker and store as soon as the lock is free.
            let why = "the lock did not come free in time; the stop stays and is written again";
            return (not_marked, Some(why.to_string()));
        };
        // A `reopen` that began first has removed the marker meanwhile, or the marker landed after
        // a `reopen` that came later: the stop in force decides which of the two it is.
        let again = match self.unsaved_mark() {
            Some(held) => timeout(wait, marker::write(path, held)).await,
            None if !self.stopped.load(Ordering::SeqCst) => {
                timeout(wait, marker::remove(path)).await
            }
            None => Ok(Ok(())),
        };
        if let Ok(result) = again {
            not_marked = result.err();
        }
        let not_persisted = match self.view().await {
            View::Doc(doc) => self.save(&doc).await.err(),
            // Nothing to keep: a missing state reads as stopped after a restart too.
            View::Missing => None,
            View::Unreadable(why) | View::Corrupt(why) => Some(format!(
                "the state cannot be read, so it reads as stopped: {why}"
            )),
        };
        (not_marked, not_persisted)
    }

    async fn settle_all(self: Arc<Self>) -> (usize, Option<String>) {
        let Some(port) = self.port.get() else {
            return (0, None);
        };
        match port.settle_open_calls(None).await {
            Ok(found) if found.unreadable > 0 => (
                found.settled,
                Some(format!("{} messages could not be read", found.unreadable)),
            ),
            Ok(found) => (found.settled, None),
            Err(e) => (0, Some(e.to_string())),
        }
    }

    async fn end_turns(self: Arc<Self>) -> Result<Vec<fs_messenger_core::TurnStop>, String> {
        let Some(port) = self.port.get() else {
            return Ok(vec![]);
        };
        port.stop_turns(None, self.config.turn_wait)
            .await
            .map_err(|e| e.to_string())
    }
}
