//! `MessageAction.decide` on the engine.
//!
//! Pattern: Template method over the pure `GuardDoc::apply_decision`: on a copy, written whole or not
//! at all. Nothing but a written decision changes anything; a refusal touches neither the state nor
//! the log. What a message shows is written afterwards and decides nothing.

use fs_messenger_core::{ToolCallChoice, ToolCallDecision};
use fs_store::ObjectId;

use super::core::GuardEngine;
use crate::error::GuardError;
use crate::log::GuardLogEntry;
use crate::state::DecideInput;

fn choice_name(choice: ToolCallChoice) -> &'static str {
    match choice {
        ToolCallChoice::Refuse => "refuse",
        ToolCallChoice::AllowOnce => "allowOnce",
        ToolCallChoice::AllowClass => "allowClass",
    }
}

impl GuardEngine {
    pub(super) async fn decide_call(
        &self,
        conversation: ObjectId,
        message: ObjectId,
        decision: &ToolCallDecision,
    ) -> Result<(), GuardError> {
        let _ops = self.ops.lock().await;
        let mut doc = self.open_doc().await?;
        let limits = self.limits();
        let input = DecideInput {
            conversation,
            message,
            decision,
            now: self.clock.now(),
            limits: &limits,
        };
        let decided = doc.apply_decision(&input, self.verifier.as_ref())?;
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        let mut problems = vec![];
        if let Err(why) = self.show(&decided.call, "").await {
            problems.push(why);
        }
        let mut entry = GuardLogEntry::new("decide", &decision.signer).about(&decided.call.caller);
        entry.call.clone_from(&decided.call.id);
        entry.capability.clone_from(&decided.call.capability);
        entry.choice = choice_name(decided.choice).to_string();
        entry.risk = decided.granted.as_ref().map(|g| g.risk);
        if let Err(why) = self.repo.append(&entry).await {
            problems.push(why);
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(GuardError::DoneNotShown(problems.join("; ")))
        }
    }
}
