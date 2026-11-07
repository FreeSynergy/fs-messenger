//! [`Gate`] on the engine: where anyone who wants to act comes in.
//!
//! Pattern: Facade over the pure `GuardDoc::evaluate`: read, evaluate, then do what it says
//! under one lock, and look at the latch once more before anything is allowed. An `allowOnce` is used
//! up in the state before the answer goes out; if that cannot be written the answer is no.

use std::sync::PoisonError;

use async_trait::async_trait;
use fs_messenger_core::{ToolCallOutcome, ToolCallRisk};

use super::core::GuardEngine;
use super::store_access::View;
use crate::api::Gate;
use crate::arguments::Arguments;
use crate::error::Refusal;
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::model::{Allowance, Caller, GuardMode, Verdict};
use crate::sensitivity::ObjectRef;
use crate::state::{CallRecord, CallStage, Evaluation, GuardDoc, Judged};
use crate::table::{CapabilityRule, capability_rule};

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

    /// Show what the state now says. What is shown decides nothing, so a failure only misleads.
    pub(super) async fn show(&self, call: &CallRecord, detail: &str) -> Result<(), String> {
        if self.port.get().is_none() {
            return Ok(());
        }
        let words = self.destination_for(call).await;
        self.show_with(call, detail, &words).await
    }

    /// As `show`, with the destination already found: several calls of one participant and program
    /// are shown together without asking the source for each.
    pub(super) async fn show_with(
        &self,
        call: &CallRecord,
        detail: &str,
        words: &str,
    ) -> Result<(), String> {
        let Some(port) = self.port.get() else {
            return Ok(());
        };
        let update = port.update_call(call.message, call.shown(detail, words));
        tokio::time::timeout(self.config.store_wait, update)
            .await
            .map_err(|_| "the messenger did not answer in time".to_string())?
            .map_err(|e| e.to_string())
    }

    pub(super) async fn log_entry(&self, entry: &GuardLogEntry) -> Result<(), String> {
        tokio::time::timeout(self.config.store_wait, self.repo.append(entry))
            .await
            .map_err(|_| "the store did not answer in time".to_string())?
    }

    async fn use_approval(&self, mut doc: GuardDoc, at: usize) -> Verdict {
        let call = &mut doc.calls[at];
        call.stage = CallStage::Running;
        call.revision += 1;
        let call = call.clone();
        // An early look only: it spares the write. The one that counts is right before `Allow`.
        if self.is_latched() {
            return Verdict::Refuse(Refusal::Stopped);
        }
        if self.save(&doc).await.is_err() {
            return Verdict::Refuse(Refusal::NotRecorded);
        }
        let _ = self.show(&call, "").await;
        let mut entry = GuardLogEntry::new("approvalUsed", &call.caller).about(&call.caller);
        entry.call.clone_from(&call.id);
        entry.capability.clone_from(&call.capability);
        // The call was allowed and the state holds that; a log that fails does not undo it.
        let _ = self.log_entry(&entry).await;
        // The last look, after the slow steps: a stop that came in meanwhile has the call cancelled.
        if self.is_latched() {
            return Verdict::Refuse(Refusal::Stopped);
        }
        Verdict::Allow(Allowance::Once { call: call.id })
    }

    /// A bounded record of the use of a release: at most one per participant, tool and minute.
    async fn log_class_use(&self, profile: &str, capability: &str, risk: ToolCallRisk) {
        let now = self.clock.now();
        let every = chrono::Duration::from_std(self.config.class_log_every)
            .unwrap_or(chrono::TimeDelta::MAX);
        let key = (profile.to_string(), capability.to_string());
        {
            let mut logged = self
                .class_logged
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if logged.get(&key).is_some_and(|last| now < *last + every) {
                return;
            }
            logged.retain(|_, last| now < *last + every);
            logged.insert(key, now);
        }
        let mut entry = GuardLogEntry::new("classUsed", profile).about(profile);
        entry.capability = capability.to_string();
        entry.risk = Some(risk);
        let _ = self.log_entry(&entry).await;
    }
}

impl GuardEngine {
    /// `check`, with the object a read names, if it names one.
    async fn check_inner(
        &self,
        caller: &Caller,
        capability: &str,
        arguments: &Arguments,
        object: Option<&ObjectRef>,
    ) -> Verdict {
        if self.is_latched() {
            self.ensure_retry();
            return Verdict::Refuse(Refusal::Stopped);
        }
        // A read that names its object is looked up, and before the lock: the source may be slow. One
        // that names none counts as marked (strategy 4d(2)); only `check_object` can say otherwise.
        // The table decides what is a read, not whether the call has input, so that a call without
        // input is looked up too.
        let reads = capability_rule(capability) == CapabilityRule::Risk(ToolCallRisk::ReadsOnly);
        let sensitive = match object {
            Some(object) if reads => self.read_asks(object).await,
            Some(_) => false,
            None => true,
        };
        let Ok(_ops) = self.lock_ops().await else {
            return Verdict::Refuse(Refusal::Stopped);
        };
        let doc = match self.open_doc().await {
            Ok(doc) => doc,
            Err(refusal) => return Verdict::Refuse(refusal),
        };
        let profile = id_key(&caller.profile);
        let now = self.clock.now();
        let limits = self.limits();
        let turn = caller.conversation;
        let judged = Judged {
            caller: &profile,
            turn,
            capability,
            arguments,
            sensitive,
        };
        match doc.evaluate(&judged, now, &limits) {
            Evaluation::Refuse(refusal) => Verdict::Refuse(refusal),
            Evaluation::AllowClass(risk) => {
                // The log is slow, so the last look comes after it, right before `Allow`.
                self.log_class_use(&profile, capability, risk).await;
                if self.is_latched() {
                    return Verdict::Refuse(Refusal::Stopped);
                }
                Verdict::Allow(Allowance::Class(risk))
            }
            Evaluation::UseApproval(at) => self.use_approval(doc, at).await,
            Evaluation::AskAgain(at) => match self.count_retry(&doc.calls[at].id) {
                Ok(()) => self.question(&doc.calls[at], &arguments.detail()).await,
                Err(refusal) => Verdict::Refuse(refusal),
            },
            Evaluation::AskNew(input) => {
                self.ask_new(doc, &profile, capability, arguments, input)
                    .await
            }
        }
    }
}

#[async_trait]
impl Gate for GuardEngine {
    async fn check(&self, caller: &Caller, capability: &str, arguments: &Arguments) -> Verdict {
        self.check_inner(caller, capability, arguments, None).await
    }

    async fn check_object(
        &self,
        caller: &Caller,
        capability: &str,
        arguments: &Arguments,
        object: &ObjectRef,
    ) -> Verdict {
        self.check_inner(caller, capability, arguments, Some(object))
            .await
    }

    async fn revise(&self, caller: &Caller, call: &str, arguments: &Arguments) -> Verdict {
        if self.is_latched() {
            return Verdict::Refuse(Refusal::Stopped);
        }
        let Ok(_ops) = self.lock_ops().await else {
            return Verdict::Refuse(Refusal::Stopped);
        };
        let doc = match self.open_doc().await {
            Ok(doc) => doc,
            Err(refusal) => return Verdict::Refuse(refusal),
        };
        self.revise_call(doc, caller, call, arguments).await
    }

    async fn finished(
        &self,
        caller: &Caller,
        call: &str,
        outcome: ToolCallOutcome,
    ) -> Result<(), Refusal> {
        let (stage, name) = match outcome {
            ToolCallOutcome::Done => (CallStage::Done, "done"),
            ToolCallOutcome::Failed => (CallStage::Failed, "failed"),
            _ => return Err(Refusal::UnknownOption),
        };
        let _ops = self.lock_ops().await?;
        let View::Doc(mut doc) = self.view().await else {
            return Err(Refusal::Stopped);
        };
        let profile = id_key(&caller.profile);
        let record = doc
            .calls
            .iter_mut()
            .find(|c| c.id == call && c.caller == profile)
            .ok_or(Refusal::UnknownCall)?;
        if record.stage != CallStage::Running {
            return Err(Refusal::SettledCall);
        }
        record.stage = stage;
        record.revision += 1;
        let record = record.clone();
        self.save(&doc).await.map_err(|_| Refusal::NotRecorded)?;
        let _ = self.show(&record, "").await;
        let mut entry = GuardLogEntry::new("finished", &profile).about(&profile);
        entry.call.clone_from(&record.id);
        entry.capability.clone_from(&record.capability);
        entry.outcome = name.to_string();
        let _ = self.log_entry(&entry).await;
        Ok(())
    }

    async fn admit_message(&self, sender: &str) -> Result<(), Refusal> {
        self.admission(sender, self.config.store_wait).await
    }
}
