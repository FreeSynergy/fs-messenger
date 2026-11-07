//! Asking the source of destinations, with a limit, when a question is shown.
//!
//! Pattern: Facade over the Strategy [`DestinationLookup`]: one fixed way to ask it. It is asked for
//! the participant and the program of the engine's own record of the call, never for anything from the
//! call's arguments, and when a question is shown, not when it is stored. The source runs in a task of
//! its own with a time limit. No answer in time, a panic, an empty answer or one that hides text all
//! read as "unbekannt": never empty, and never an error that keeps the question from standing.

use std::sync::Arc;

use fs_messenger_core::ToolCall;

use super::core::GuardEngine;
use crate::destination::{Destination, DestinationLookup};
use crate::no_destinations::NoDestinations;
use crate::program::program_of;
use crate::state::{CallRecord, usable_name};
use crate::words::DESTINATION_UNKNOWN;

impl GuardEngine {
    /// Where the text of this participant's calls in this program goes, in words for a person.
    pub(super) async fn destination_words(&self, participant: &str, program: &str) -> String {
        let source: Arc<dyn DestinationLookup> = self
            .destinations
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(NoDestinations));
        let (participant, program) = (participant.to_string(), program.to_string());
        let mut task =
            tokio::spawn(async move { source.destination(&participant, &program).await });
        let answer = tokio::time::timeout(self.config.store_wait, &mut task).await;
        match answer {
            Ok(Ok(Destination::Known(words)))
                if !words.trim().is_empty() && usable_name(&words) =>
            {
                words
            }
            Ok(_) => DESTINATION_UNKNOWN.to_string(),
            Err(_) => {
                task.abort();
                DESTINATION_UNKNOWN.to_string()
            }
        }
    }

    /// The destination of a call: its participant and the program of its tool, both from the record.
    pub(super) async fn destination_for(&self, call: &CallRecord) -> String {
        self.destination_words(&call.caller, &program_of(&call.capability))
            .await
    }

    /// A call as shown, with the destination found now.
    pub(super) async fn shown_call(&self, call: &CallRecord, detail: &str) -> ToolCall {
        let words = self.destination_for(call).await;
        call.shown(detail, &words)
    }
}
