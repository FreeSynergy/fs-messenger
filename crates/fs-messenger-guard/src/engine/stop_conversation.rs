//! `Conversation.stop`.
//!
//! Pattern: Facade over the pure state (a fixed order of steps, not a trait with default methods): its records, what is shown, its provider, in that order, and no step's
//! failure skips the next. Needs no proof: it can only end work. A provider that cannot end a turn is
//! answered as `Unsupported`, after everything else was done.

use fs_interfaces::provider::Unsupported;
use fs_messenger_core::TurnStopOutcome;
use fs_objects::ObjectId;

use super::core::GuardEngine;
use super::store_access::View;
use crate::error::GuardError;
use crate::log::GuardLogEntry;
use crate::state::CallStage;

const ENGINE: &str = "urn:freesynergy:messenger:guard";

impl GuardEngine {
    async fn cancel_records(&self, conversation: ObjectId) -> Result<(), String> {
        let _ops = self.lock_ops().await.map_err(|e| e.to_string())?;
        let View::Doc(mut doc) = self.view().await else {
            return Ok(());
        };
        let mut changed = false;
        for call in doc
            .calls
            .iter_mut()
            .filter(|c| c.conversation == conversation)
        {
            if !call.stage.is_settled() {
                call.stage = CallStage::Cancelled;
                call.revision += 1;
                changed = true;
            }
        }
        if changed {
            self.save(&doc).await?;
        }
        Ok(())
    }

    pub(super) async fn stop_conversation_now(
        &self,
        conversation: ObjectId,
    ) -> Result<(), GuardError> {
        let not_written = self.cancel_records(conversation).await.err();
        let Some(port) = self.port.get() else {
            return Err(GuardError::NotPersisted("no messenger is attached".into()));
        };
        let settled = port.settle_open_calls(Some(conversation)).await;
        let turns = port
            .stop_turns(Some(conversation), self.config.turn_wait)
            .await;
        let mut entry = GuardLogEntry::new("turnStop", ENGINE);
        entry.cancelled = settled.as_ref().map_or(0, |found| found.settled);
        let not_logged = self.log_entry(&entry).await.err();

        if let Some(why) = not_written {
            return Err(GuardError::NotPersisted(why));
        }
        let settled = settled.map_err(|e| GuardError::DoneNotShown(e.to_string()));
        let turns = turns.map_err(|e| GuardError::DoneNotShown(e.to_string()))?;
        settled?;
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
