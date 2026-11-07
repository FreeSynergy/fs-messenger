//! What the rules say about a participant's call before anyone is asked: the one place `check` and
//! `revise` both go through (F1 of the review of 9ea2830).
//!
//! Pattern: Specification, stretched: pure rules over the state and a call, one built from the other
//! (`standing` after `head`), with no side effect. They answer with a verdict type and not with a
//! `bool`, as `check_proof` does (open point 0). Because `revise` asks the same two functions as
//! `check`, it can never give a verdict that `check` would have refused, and it never allows: at most
//! it keeps a question.

use fs_messenger_core::ToolCallRisk;

use super::admin::usable_name;
use super::doc::{CallRecord, GuardDoc, Invitation};
use super::stage::derived_risk;
use crate::error::Refusal;
use crate::model::GuardMode;
use crate::program::program_of;
use crate::risk::can_be_released;
use crate::table::{CapabilityRule, capability_rule};

/// What a participant's call stands at, when no person has said anything about this very call.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    /// No. `Refusal::NotReleased` when nothing in the program is released.
    Refused(Refusal),
    /// The class is released for this participant and program, and the read is not marked.
    Released(ToolCallRisk),
    /// Something is released in the program, and this call asks every time.
    AsksEachTime,
}

/// Whether the call counts as a read of a marked object: only a read can be marked, so `flagged` (the
/// engine's own answer, or "no object named") counts for a read of the table and for nothing else.
pub(crate) fn counts_as_marked(capability: &str, flagged: bool) -> bool {
    flagged && capability_rule(capability) == CapabilityRule::Risk(ToolCallRisk::ReadsOnly)
}

impl GuardDoc {
    /// Stopped, a name that is no name, what is never for an AI, and the invitation, in this order.
    pub(crate) fn head(&self, caller: &str, capability: &str) -> Result<&Invitation, Refusal> {
        if self.mode != GuardMode::Open {
            return Err(Refusal::Stopped);
        }
        if !usable_name(capability) {
            return Err(Refusal::BadName);
        }
        if capability_rule(capability) == CapabilityRule::NeverForAi {
            return Err(Refusal::NeverForAi);
        }
        self.invited.get(caller).ok_or(Refusal::NotInvited)
    }

    /// Whether a call that a person answered may still go on: the same `standing` as when a call
    /// comes in, with the call counted as a marked read where it is a read (it was asked, so it
    /// asked every time). Only "no" matters here; whether a release covers it changes nothing. A
    /// question or approval that a version before releases per program left behind is refused.
    pub(crate) fn approval_standing(&self, call: &CallRecord) -> Result<(), Refusal> {
        let marked = counts_as_marked(&call.capability, true);
        match self.standing(&call.caller, &call.capability, call.has_input, marked) {
            Standing::Refused(refusal) => Err(refusal),
            Standing::Released(_) | Standing::AsksEachTime => Ok(()),
        }
    }

    /// Nothing without a release (ADR 0054): what a release could cover is refused until it is given,
    /// and what always asks (a marked read, `destroys`, `actsForPerson`, `unknown`) is asked only once
    /// something is released in this program. Nobody is asked otherwise, so a participant cannot flood
    /// a person with questions. `marked` is the answer of `counts_as_marked`.
    pub(crate) fn standing(
        &self,
        caller: &str,
        capability: &str,
        has_input: bool,
        marked: bool,
    ) -> Standing {
        let risk = derived_risk(capability, has_input);
        let program = program_of(capability);
        if !marked && self.is_released(caller, risk, &program) {
            return Standing::Released(risk);
        }
        let asks_always = marked || !can_be_released(risk);
        if asks_always && self.has_release_in(caller, &program) {
            Standing::AsksEachTime
        } else {
            Standing::Refused(Refusal::NotReleased)
        }
    }
}
