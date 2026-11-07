//! Asking the source of destinations, with a limit, when a question is shown.
//!
//! Pattern: Facade over the Strategy [`DestinationLookup`] (through the Decorator `OnceLookup`): one
//! fixed way to ask it. It is asked for the participant and the program of the engine's own record of
//! the call, never for anything from the call's arguments, and when a question is shown, not when it
//! is stored and not when expired questions are cleared away (those need no destination). Each
//! operation makes one `OnceLookup`: one question per participant and program and one wait for all
//! of it. No answer in time, a panic, an empty answer or one that hides text all read as
//! "unbekannt": never empty, and never an error that keeps the question from standing.

use std::sync::Arc;

use fs_messenger_core::ToolCall;

use super::core::GuardEngine;
use super::lookup_once::OnceLookup;
use crate::destination::{Destination, DestinationLookup};
use crate::no_destinations::NoDestinations;
use crate::program::program_of;
use crate::state::{CallRecord, usable_name};
use crate::words::DESTINATION_UNKNOWN;

impl GuardEngine {
    /// The questions of one operation to the source of destinations: asked once, one wait.
    pub(super) fn look(&self) -> OnceLookup {
        let source: Arc<dyn DestinationLookup> = self
            .destinations
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(NoDestinations));
        OnceLookup::new(source, self.config.store_wait)
    }

    /// Where the text of this participant's calls in this program goes, in words for a person.
    pub(super) async fn destination_words(
        &self,
        look: &OnceLookup,
        participant: &str,
        program: &str,
    ) -> String {
        match look.destination(participant, program).await {
            Destination::Known(words) if !words.trim().is_empty() && usable_name(&words) => words,
            _ => DESTINATION_UNKNOWN.to_string(),
        }
    }

    /// The destination of a call: its participant and the program of its tool, both from the record.
    pub(super) async fn destination_for(&self, look: &OnceLookup, call: &CallRecord) -> String {
        self.destination_words(look, &call.caller, &program_of(&call.capability))
            .await
    }

    /// A call as shown, with the destination found now.
    pub(super) async fn shown_call(
        &self,
        look: &OnceLookup,
        call: &CallRecord,
        detail: &str,
    ) -> ToolCall {
        let words = self.destination_for(look, call).await;
        call.shown(detail, &words)
    }
}
