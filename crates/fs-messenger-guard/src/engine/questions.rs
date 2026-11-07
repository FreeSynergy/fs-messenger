//! Putting a question to a person, and what limits that.
//!
//! Pattern: Facade over the pure state (a fixed order of steps, not a trait with default methods): the record goes in only after the question stands in the messenger, and
//! if it cannot be written the question is taken back. The limits are in `DESIGN.md`: a bare
//! `input_required` can be retried at once and for ever, and new input can come as fast.

use std::sync::PoisonError;

use fs_store::ObjectId;

use super::core::{GuardEngine, Retries};
use crate::arguments::Arguments;
use crate::error::Refusal;
use crate::identity::id_key;
use crate::model::{Caller, Question, Verdict};
use crate::state::{CallRecord, CallStage, GuardDoc, QuestionInput};

impl GuardEngine {
    pub(super) fn question(&self, call: &CallRecord, detail: &str) -> Verdict {
        Verdict::Ask(Box::new(Question {
            call: call.shown(detail),
            conversation: call.conversation,
            message: call.message,
            retry_after: self.config.retry_after,
        }))
    }

    /// Applies only to the same question: an approved one always goes through. Refusing here leaves
    /// the question open for the person.
    pub(super) fn count_retry(&self, id: &str) -> Result<(), Refusal> {
        let now = self.clock.now();
        let mut retries = self.retries.lock().unwrap_or_else(PoisonError::into_inner);
        let held = retries.entry(id.to_string()).or_insert(Retries {
            count: 0,
            last: chrono::DateTime::<chrono::Utc>::MIN_UTC,
        });
        held.count = held.count.saturating_add(1);
        if held.count > self.config.max_retries {
            return Err(Refusal::RetriesExhausted);
        }
        let wait = chrono::Duration::from_std(self.config.retry_after)
            .unwrap_or(chrono::TimeDelta::zero());
        if now < held.last + wait {
            return Err(Refusal::RetryTooSoon);
        }
        held.last = now;
        Ok(())
    }

    async fn sweep(&self, doc: &mut GuardDoc) {
        for id in doc.sweep_expired(self.clock.now()) {
            if let Some(call) = doc.calls.iter().find(|c| c.id == id) {
                // Shown as open until this works; a click on it is refused either way.
                let _ = self.show(call, "").await;
            }
        }
    }

    pub(super) async fn ask_new(
        &self,
        mut doc: GuardDoc,
        profile: &str,
        capability: &str,
        arguments: &Arguments,
        input: QuestionInput,
    ) -> Verdict {
        let Some(port) = self.port.get() else {
            return Verdict::Refuse(Refusal::NoWayToAsk);
        };
        self.sweep(&mut doc).await;
        let now = self.clock.now();
        let mut call = CallRecord {
            id: ObjectId::new().to_string(),
            caller: profile.to_string(),
            capability: capability.to_string(),
            digest: input.digest,
            has_input: input.has_input,
            revision: 1,
            turn: input.turn,
            stage: CallStage::Question,
            asked_of: input.asked_of,
            conversation: ObjectId::new(),
            message: ObjectId::new(),
            expires_at: now + self.limits().question_ttl,
        };
        let detail = arguments.detail();
        let placing = port.place_question(input.turn, profile, call.shown(&detail));
        let placed = tokio::time::timeout(self.config.store_wait, placing).await;
        let Ok(Ok(placed)) = placed else {
            return Verdict::Refuse(Refusal::NoWayToAsk);
        };
        call.conversation = placed.conversation;
        call.message = placed.message;
        doc.calls.push(call.clone());
        doc.count_question(profile, now);
        let pruned = doc.prune_settled();
        if self.is_latched() || self.save(&doc).await.is_err() {
            // A question without a record can be clicked, and the click is refused. Still, say so.
            call.stage = CallStage::Cancelled;
            call.revision += 1;
            let _ = self.show(&call, &detail).await;
            return Verdict::Refuse(Refusal::NotRecorded);
        }
        for message in pruned {
            let _ = port.delete_settled_question(message).await;
        }
        let count = Retries {
            count: 0,
            last: now,
        };
        self.retries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(call.id.clone(), count);
        self.question(&call, &detail)
    }

    /// New input has the same limit as a retry, and does not make the card live longer.
    pub(super) async fn revise_call(
        &self,
        mut doc: GuardDoc,
        caller: &Caller,
        call: &str,
        arguments: &Arguments,
    ) -> Verdict {
        let profile = id_key(&caller.profile);
        let held = doc
            .calls
            .iter()
            .position(|c| c.id == call && c.caller == profile && c.turn == caller.conversation);
        let Some(at) = held else {
            return Verdict::Refuse(Refusal::UnknownCall);
        };
        if let Err(refusal) = self.count_retry(&doc.calls[at].id) {
            return Verdict::Refuse(refusal);
        }
        match doc.apply_revise(at, arguments) {
            Err(refusal) => Verdict::Refuse(refusal),
            Ok(false) => self.question(&doc.calls[at], &arguments.detail()),
            Ok(true) => {
                if self.save(&doc).await.is_err() {
                    return Verdict::Refuse(Refusal::NotRecorded);
                }
                let detail = arguments.detail();
                let _ = self.show(&doc.calls[at], &detail).await;
                self.question(&doc.calls[at], &detail)
            }
        }
    }
}
