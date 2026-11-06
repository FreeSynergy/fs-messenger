//! [`Gate`] on the engine: where anyone who wants to act comes in.
//!
//! Pattern: Template method over the pure `GuardDoc::evaluate`: read, evaluate, then do what it says
//! under one lock, and look at the latch once more before anything is allowed. An `allowOnce` is used
//! up in the state before the answer goes out; if that cannot be written the answer is no.
//! Retries: see `Retries`, and `DESIGN.md` for the numbers.

use std::sync::PoisonError;

use async_trait::async_trait;
use fs_messenger_core::ToolCallOutcome;
use fs_store::ObjectId;

use super::core::{GuardEngine, Retries, View};
use crate::api::Gate;
use crate::arguments::Arguments;
use crate::error::Refusal;
use crate::model::{Allowance, Caller, GuardMode, Question, Verdict};
use crate::state::{CallRecord, CallStage, Evaluation, GuardDoc, QuestionInput};

impl GuardEngine {
    /// The state, if it allows anything at all.
    pub(super) async fn open_doc(&self) -> Result<GuardDoc, Refusal> {
        if self.is_latched() {
            return Err(Refusal::Stopped);
        }
        match self.view().await {
            View::Doc(doc) if doc.mode == GuardMode::Open => Ok(*doc),
            _ => Err(Refusal::Stopped),
        }
    }

    fn question(&self, call: &CallRecord, detail: &str) -> Verdict {
        Verdict::Ask(Box::new(Question {
            call: call.shown(detail),
            conversation: call.conversation,
            message: call.message,
            retry_after: self.config.retry_after,
        }))
    }

    /// A bare `input_required` can be retried at once and for ever. Applies only to the same question:
    /// an approved one always goes through. Refusing here leaves the question open for the person.
    fn count_retry(&self, id: &str) -> Result<(), Refusal> {
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

    /// Show what the state now says. What is shown decides nothing, so a failure only misleads.
    pub(super) async fn show(&self, call: &CallRecord, detail: &str) -> Result<(), String> {
        let Some(port) = self.port.get() else {
            return Ok(());
        };
        port.update_call(call.message, call.shown(detail))
            .await
            .map_err(|e| e.to_string())
    }

    async fn sweep(&self, doc: &mut GuardDoc) {
        for id in doc.sweep_expired(self.clock.now()) {
            if let Some(call) = doc.calls.iter().find(|c| c.id == id) {
                // Shown as open until this works; a click on it is refused either way.
                let _ = self.show(call, "").await;
            }
        }
    }

    async fn ask_new(
        &self,
        mut doc: GuardDoc,
        caller: &Caller,
        capability: &str,
        arguments: &Arguments,
        input: QuestionInput,
    ) -> Verdict {
        let Some(port) = self.port.get() else {
            return Verdict::Refuse(Refusal::NoWayToAsk);
        };
        self.sweep(&mut doc).await;
        let mut call = CallRecord {
            id: ObjectId::new().to_string(),
            caller: caller.profile.clone(),
            capability: capability.to_string(),
            digest: input.digest,
            has_input: input.has_input,
            revision: 1,
            stage: CallStage::Question,
            asked_of: input.asked_of,
            conversation: ObjectId::new(),
            message: ObjectId::new(),
            expires_at: self.clock.now() + self.limits().question_ttl,
        };
        let detail = arguments.detail();
        let placed = port
            .place_question(caller.conversation, &caller.profile, call.shown(&detail))
            .await;
        let Ok(placed) = placed else {
            return Verdict::Refuse(Refusal::NoWayToAsk);
        };
        call.conversation = placed.conversation;
        call.message = placed.message;
        doc.calls.push(call.clone());
        doc.prune_settled();
        if self.save(&doc).await.is_err() {
            // A question without a record can be clicked, and the click is refused. Still, say so.
            call.stage = CallStage::Cancelled;
            call.revision += 1;
            let _ = self.show(&call, &detail).await;
            return Verdict::Refuse(Refusal::NotRecorded);
        }
        let count = Retries {
            count: 0,
            last: self.clock.now(),
        };
        self.retries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(call.id.clone(), count);
        self.question(&call, &detail)
    }

    async fn use_approval(&self, mut doc: GuardDoc, at: usize) -> Verdict {
        let call = &mut doc.calls[at];
        call.stage = CallStage::Running;
        call.revision += 1;
        let call = call.clone();
        // The last look: a stop that came in meanwhile wins.
        if self.is_latched() {
            return Verdict::Refuse(Refusal::Stopped);
        }
        if self.save(&doc).await.is_err() {
            return Verdict::Refuse(Refusal::NotRecorded);
        }
        let _ = self.show(&call, "").await;
        Verdict::Allow(Allowance::Once { call: call.id })
    }
}

#[async_trait]
impl Gate for GuardEngine {
    async fn check(&self, caller: &Caller, capability: &str, arguments: &Arguments) -> Verdict {
        if self.is_latched() {
            self.flush_stop().await;
            return Verdict::Refuse(Refusal::Stopped);
        }
        let _ops = self.ops.lock().await;
        let doc = match self.open_doc().await {
            Ok(doc) => doc,
            Err(refusal) => return Verdict::Refuse(refusal),
        };
        let now = self.clock.now();
        let limits = self.limits();
        match doc.evaluate(&caller.profile, capability, arguments, now, &limits) {
            Evaluation::Refuse(refusal) => Verdict::Refuse(refusal),
            Evaluation::AllowClass(risk) => {
                if self.is_latched() {
                    return Verdict::Refuse(Refusal::Stopped);
                }
                Verdict::Allow(Allowance::Class(risk))
            }
            Evaluation::UseApproval(at) => self.use_approval(doc, at).await,
            Evaluation::AskAgain(at) => match self.count_retry(&doc.calls[at].id) {
                Ok(()) => self.question(&doc.calls[at], &arguments.detail()),
                Err(refusal) => Verdict::Refuse(refusal),
            },
            Evaluation::AskNew(input) => {
                self.ask_new(doc, caller, capability, arguments, input)
                    .await
            }
        }
    }

    async fn revise(&self, caller: &Caller, call: &str, arguments: &Arguments) -> Verdict {
        if self.is_latched() {
            return Verdict::Refuse(Refusal::Stopped);
        }
        let _ops = self.ops.lock().await;
        let mut doc = match self.open_doc().await {
            Ok(doc) => doc,
            Err(refusal) => return Verdict::Refuse(refusal),
        };
        let held = doc
            .calls
            .iter()
            .position(|c| c.id == call && c.caller == caller.profile);
        let Some(at) = held else {
            return Verdict::Refuse(Refusal::UnknownCall);
        };
        let limits = self.limits();
        match doc.apply_revise(at, arguments, self.clock.now(), &limits) {
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

    async fn finished(
        &self,
        caller: &Caller,
        call: &str,
        outcome: ToolCallOutcome,
    ) -> Result<(), Refusal> {
        let stage = match outcome {
            ToolCallOutcome::Done => CallStage::Done,
            ToolCallOutcome::Failed => CallStage::Failed,
            _ => return Err(Refusal::UnknownOption),
        };
        let _ops = self.ops.lock().await;
        let View::Doc(mut doc) = self.view().await else {
            return Err(Refusal::Stopped);
        };
        let record = doc
            .calls
            .iter_mut()
            .find(|c| c.id == call && c.caller == caller.profile)
            .ok_or(Refusal::UnknownCall)?;
        if record.stage != CallStage::Running {
            return Err(Refusal::SettledCall);
        }
        record.stage = stage;
        record.revision += 1;
        let record = record.clone();
        doc.prune_settled();
        self.save(&doc).await.map_err(|_| Refusal::NotRecorded)?;
        let _ = self.show(&record, "").await;
        Ok(())
    }

    async fn admit_message(&self, sender: &str) -> Result<(), Refusal> {
        let doc = self.open_doc().await?;
        if doc.is_invited(sender) {
            Ok(())
        } else {
            Err(Refusal::NotInvited)
        }
    }
}
