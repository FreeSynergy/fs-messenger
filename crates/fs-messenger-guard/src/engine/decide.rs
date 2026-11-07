//! `MessageAction.decide` on the engine.
//!
//! Pattern: Facade over the pure `GuardDoc::apply_decision`: on a copy, written whole or not
//! at all. Nothing but a written decision changes anything; a refusal touches neither the state nor
//! the log. What a message shows is written afterwards and decides nothing. On a stale or settled
//! refusal listeners hear how the call stands, from memory only.

use fs_messenger_core::{DecisionProblem, ToolCallDecision};
use fs_objects::ObjectId;

use super::core::GuardEngine;
use super::lookup_once::OnceLookup;
use super::store_access::View;
use crate::error::{GuardError, Refusal};
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::model::GuardMode;
use crate::state::{DecideInput, GuardDoc};

impl GuardEngine {
    /// Stale and settled are answered with the state as it really is, so a card can show it. Nothing
    /// is written; if it cannot be told, the card still has the refusal.
    async fn announce_on_refusal(
        &self,
        doc: &GuardDoc,
        decision: &ToolCallDecision,
        refusal: &Refusal,
        look: &OnceLookup,
    ) {
        if !matches!(
            refusal.decision_problem(),
            DecisionProblem::Stale | DecisionProblem::Settled
        ) {
            return;
        }
        let call = doc.calls.iter().find(|c| c.id == decision.tool_call);
        if let (Some(call), Some(port)) = (call, self.port.get()) {
            let shown = self.shown_call(look, call, "").await;
            let _ = port
                .announce_call(call.conversation, call.message, shown)
                .await;
        }
    }

    pub(super) async fn decide_call(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        decision: &ToolCallDecision,
    ) -> Result<(), GuardError> {
        let _ops = self.lock_ops().await?;
        // Not `open_doc`: a stopped program still says how a call ended.
        let View::Doc(mut doc) = self.view().await else {
            return Err(Refusal::Damaged.into());
        };
        if self.is_latched() {
            doc.mode = GuardMode::Stopped;
        }
        let look = self.look();
        let limits = self.limits();
        let input = DecideInput {
            conversation,
            message,
            decision,
            now: self.clock.now(),
            limits: &limits,
        };
        let decided = match doc.apply_decision(&input, self.verifier.as_ref()) {
            Ok(decided) => decided,
            Err(refusal) => {
                self.announce_on_refusal(&doc, decision, &refusal, &look)
                    .await;
                return Err(refusal.into());
            }
        };
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        let mut problems = vec![];
        if let Err(why) = self.show(&look, &decided.call, "").await {
            problems.push(why);
        }
        let signer = id_key(&decision.signer);
        let mut entry = GuardLogEntry::new("decide", &signer).about(&decided.call.caller);
        entry.call.clone_from(&decided.call.id);
        entry.capability.clone_from(&decided.call.capability);
        entry.choice = decided.answer.name().to_string();
        if let Err(why) = self.log_entry(&entry).await {
            problems.push(why);
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(GuardError::DoneNotShown(problems.join("; ")))
        }
    }
}
