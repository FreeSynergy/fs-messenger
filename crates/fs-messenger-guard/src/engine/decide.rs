//! `MessageAction.decide` on the engine.
//!
//! Pattern: Facade over the pure `GuardDoc::apply_decision`: on a copy, written whole or not
//! at all. Nothing but a written decision changes anything; a refusal touches neither the state nor
//! the log. What a message shows is written afterwards and decides nothing. On a stale or settled
//! refusal listeners hear how the call stands, from memory only, (an open question is looked up outside the lock, the rest shows the destination as unknown).

use fs_messenger_core::{DecisionProblem, ToolCallDecision};
use fs_objects::ObjectId;

use super::core::GuardEngine;
use super::lookup_once::OnceLookup;
use super::store_access::View;
use crate::error::{GuardError, Refusal};
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::model::GuardMode;
use crate::state::{CallStage, DecideInput, GuardDoc};
use crate::words::DESTINATION_UNKNOWN;

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
            // The engine lock is already let go (see `decide_call`): this runs before the signature is
            // checked, and a slow source of destinations must not hold the lock for one wait per
            // refused click (open point 17). Only a card that is still an open question shows where
            // the text goes (the person decides by it), so only that one is looked up, and a stale
            // click must not wipe it; a settled or working card shows "unknown" without a lookup.
            let shown = if call.stage == CallStage::Question {
                self.shown_call(look, call, "").await
            } else {
                call.shown("", DESTINATION_UNKNOWN)
            };
            // Back under the lock, for the announce only (no lookup): if the call changed while the
            // destination was looked up, a card of the old state must not replace the new one.
            let Ok(_ops) = self.lock_ops().await else {
                return;
            };
            let View::Doc(now) = self.view().await else {
                return;
            };
            let unchanged = now
                .calls
                .iter()
                .any(|c| c.id == call.id && c.stage == call.stage && c.revision == call.revision);
            if unchanged {
                let _ = port
                    .announce_call(call.conversation, call.message, shown)
                    .await;
            }
        }
    }

    pub(super) async fn decide_call(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        decision: &ToolCallDecision,
    ) -> Result<(), GuardError> {
        let ops = self.lock_ops().await?;
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
                // Not under the lock: the lookup of a destination may wait.
                drop(ops);
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
