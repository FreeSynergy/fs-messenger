//! The emergency stop, for the whole program and for one conversation.
//!
//! Pattern: Template method with a fixed order, which is the guarantee: 1. check who pulls it; 2. latch
//! in memory, from where `check` refuses, without a lock; 3. write it; 4. settle what is shown; 5. ask
//! the providers, each in its own task and without any lock; 6. log. Every step after the latch reports
//! its failure and none of them can undo the one before.
//!
//! A state that cannot be read holds no keys to check a proof against. It reads as stopped already, so
//! such a pull goes on without a proof, and can do nothing but end work.

use fs_interfaces::provider::Unsupported;
use fs_messenger_core::TurnStopOutcome;
use fs_store::ObjectId;

use super::core::{GuardEngine, View};
use crate::error::{GuardError, Refusal};
use crate::log::GuardLogEntry;
use crate::model::{GuardProof, StopReport};
use crate::signed::Method;
use crate::state::{ProofRequest, StopMark};

impl GuardEngine {
    pub(super) async fn pull_stop(&self, proof: &GuardProof) -> Result<StopReport, GuardError> {
        let view = self.view().await;
        let mut used = None;
        let by = if let View::Doc(doc) = &view {
            let request = ProofRequest {
                method: Method::Stop,
                arguments: &[],
            };
            used = Some(doc.check_proof(self.verifier.as_ref(), &request, proof)?);
            if !self.permission.may_stop(&proof.signer, &self.config.owner) {
                return Err(Refusal::NotPermitted.into());
            }
            proof.signer.clone()
        } else {
            String::new()
        };
        let mark = StopMark {
            by,
            at: self.clock.now(),
        };
        self.mark_unsaved(mark.clone());

        let not_persisted = self.persist_stop(&mark, used).await;
        self.retries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();

        let mut report = StopReport {
            not_persisted,
            calls_cancelled: 0,
            not_shown: None,
            turns: vec![],
            not_logged: None,
            messenger_attached: self.port.get().is_some(),
        };
        if let Some(port) = self.port.get() {
            match port.settle_open_calls(None).await {
                Ok(found) => {
                    report.calls_cancelled = found.settled;
                    if found.unreadable > 0 {
                        report.not_shown =
                            Some(format!("{} messages could not be read", found.unreadable));
                    }
                }
                Err(e) => report.not_shown = Some(e.to_string()),
            }
            match port.stop_turns(None, self.config.turn_wait).await {
                Ok(turns) => report.turns = turns,
                Err(e) => report.not_shown = Some(e.to_string()),
            }
        }
        let mut entry = GuardLogEntry::new("stop", &mark.by);
        entry.cancelled = report.calls_cancelled;
        if entry.actor.is_empty() {
            entry.actor = "urn:freesynergy:messenger:guard".to_string();
        }
        report.not_logged = self.repo.append(&entry).await.err();
        Ok(report)
    }

    /// Write the stop. Answers why that failed, if it did. The lock may be held by a `reopen` that
    /// began first; it clears the latch when it ends, so the latch is set again here, under the lock.
    async fn persist_stop(&self, mark: &StopMark, used: Option<String>) -> Option<String> {
        let _ops = self.ops.lock().await;
        self.mark_unsaved(mark.clone());
        match self.view().await {
            View::Doc(mut doc) => {
                if let Some(used) = used {
                    doc.used_proofs.insert(used);
                }
                self.save(&doc).await.err()
            }
            // Nothing to keep: a missing state reads as stopped after a restart too.
            View::Missing => None,
            View::Unreadable(why) | View::Corrupt(why) => Some(format!(
                "the state cannot be read, so it reads as stopped: {why}"
            )),
        }
    }

    /// `Conversation.stop`. Needs no proof: it can only end work. Open calls here are settled; a
    /// provider that cannot end a turn is answered as `Unsupported`, after everything else was done.
    pub(super) async fn stop_conversation_now(
        &self,
        conversation: ObjectId,
    ) -> Result<(), GuardError> {
        let mut problem = None;
        {
            let _ops = self.ops.lock().await;
            if let View::Doc(mut doc) = self.view().await {
                let mut changed = false;
                for call in doc
                    .calls
                    .iter_mut()
                    .filter(|c| c.conversation == conversation)
                {
                    if !call.stage.is_settled() {
                        call.stage = crate::state::CallStage::Cancelled;
                        call.revision += 1;
                        changed = true;
                    }
                }
                if changed {
                    problem = self.save(&doc).await.err();
                }
            }
        }
        let Some(port) = self.port.get() else {
            return Err(Refusal::NoWayToAsk.into());
        };
        let found = port
            .settle_open_calls(Some(conversation))
            .await
            .map_err(|e| GuardError::DoneNotShown(e.to_string()))?;
        let turns = port
            .stop_turns(Some(conversation), self.config.turn_wait)
            .await
            .map_err(|e| GuardError::DoneNotShown(e.to_string()))?;
        let mut entry = GuardLogEntry::new("turnStop", "urn:freesynergy:messenger:guard");
        entry.cancelled = found.settled;
        let not_logged = self.repo.append(&entry).await.err();
        if let Some(why) = problem {
            return Err(GuardError::NotPersisted(why));
        }
        if let Some(why) = not_logged {
            return Err(GuardError::DoneNotShown(why));
        }
        for turn in turns {
            match turn.outcome {
                TurnStopOutcome::Ended => {}
                TurnStopOutcome::Unsupported(why) => {
                    let why = format!(
                        "open calls here are cancelled, but this provider cannot end a turn: {why}"
                    );
                    return Err(Unsupported::new("turn.stop", why).into());
                }
                TurnStopOutcome::Failed(why) => return Err(GuardError::DoneNotShown(why)),
                TurnStopOutcome::NoAnswer => {
                    let why = "open calls here are cancelled, but the provider did not answer";
                    return Err(GuardError::DoneNotShown(why.to_string()));
                }
            }
        }
        Ok(())
    }
}
