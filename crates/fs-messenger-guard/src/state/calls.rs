//! `check` as a pure function: what the state says about one call.
//!
//! Pattern: State (the rules are pure transitions on `CallStage`): the order of the rules is the guarantee. Stopped first, then what is
//! never for an AI, then the invitation (`head`). Then the exact arguments a person approved, then a release,
//! and only then a question. Between the release and the question comes "nothing without a release"
//! (`standing`): what a release could cover is refused, not asked, until it is given, and what always
//! asks is asked only once something is released in the program (ADR 0054). `revise` goes through the
//! same two rules. The risk comes from the table, and no input makes it `unknown`.

use chrono::{DateTime, Duration, Utc};
use fs_messenger_core::ToolCallRisk;
use fs_store::ObjectId;

use super::doc::{CallRecord, GuardDoc};
use super::stage::CallStage;
use super::standing::{Standing, counts_as_marked};
use crate::arguments::Arguments;
use crate::digest::{arguments_digest, hex};
use crate::error::Refusal;

/// The bounds of questions. Provisional; `DESIGN.md` states the numbers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) question_ttl: Duration,
    pub(crate) approval_ttl: Duration,
    pub(crate) open_per_participant: usize,
    pub(crate) refusal_pause: Duration,
    pub(crate) questions_per_hour: u32,
}

/// What `check` has to do next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Evaluation {
    Refuse(Refusal),
    AllowClass(ToolCallRisk),
    /// This call was approved for exactly these arguments: use it up.
    UseApproval(usize),
    /// A question for exactly these arguments stands.
    AskAgain(usize),
    /// Put a new question.
    AskNew(QuestionInput),
}

/// What a new question is about.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct QuestionInput {
    pub(crate) digest: String,
    pub(crate) has_input: bool,
    pub(crate) asked_of: String,
    pub(crate) turn: Option<ObjectId>,
    pub(crate) sensitive: bool,
}

/// One call to judge. `sensitive` is what the engine's source said of the object that is read (marked
/// or not known); nothing in `arguments` can set or clear it.
pub(crate) struct Judged<'a> {
    pub(crate) caller: &'a str,
    pub(crate) turn: Option<ObjectId>,
    pub(crate) capability: &'a str,
    pub(crate) arguments: &'a Arguments,
    pub(crate) sensitive: bool,
}

/// More new input than this for one question is no longer a person's question. Persisted with the
/// revision, so a restart does not refill it.
const MAX_REVISION: u64 = 16;

impl GuardDoc {
    /// The call's own digest, for this caller. A question that has expired no longer matches.
    fn matching(&self, caller: &str, digest: &str, now: DateTime<Utc>) -> Option<usize> {
        self.calls.iter().position(|c| {
            c.caller == caller
                && c.digest == digest
                && matches!(c.stage, CallStage::Question | CallStage::Approved)
                && c.expires_at > now
        })
    }

    /// What the state says. Changes nothing.
    pub(crate) fn evaluate(
        &self,
        call: &Judged<'_>,
        now: DateTime<Utc>,
        limits: &Limits,
    ) -> Evaluation {
        let Judged {
            caller,
            turn,
            capability,
            arguments,
            sensitive,
        } = *call;
        let invitation = match self.head(caller, capability) {
            Ok(invitation) => invitation,
            Err(refusal) => return Evaluation::Refuse(refusal),
        };
        let digest = hex(&arguments_digest(caller, turn, capability, arguments));
        let held = self.matching(caller, &digest, now);
        if let Some(at) = held {
            if self.calls[at].stage == CallStage::Approved {
                return Evaluation::UseApproval(at);
            }
        }
        // Only a read can be marked. A marked read is never covered by a release of its class.
        let sensitive = counts_as_marked(capability, sensitive);
        match self.standing(caller, capability, arguments.has_input(), sensitive) {
            Standing::Refused(refusal) => return Evaluation::Refuse(refusal),
            Standing::Released(risk) => return Evaluation::AllowClass(risk),
            Standing::AsksEachTime => {}
        }
        if let Some(at) = held {
            return Evaluation::AskAgain(at);
        }
        if invitation.paused_until.is_some_and(|until| until > now) {
            return Evaluation::Refuse(Refusal::Paused);
        }
        let hour_on = invitation
            .asked_since
            .is_some_and(|since| now < since + Duration::hours(1));
        if hour_on && invitation.asked >= limits.questions_per_hour {
            return Evaluation::Refuse(Refusal::TooManyAsked);
        }
        let open = self
            .calls
            .iter()
            .filter(|c| c.caller == caller && is_waiting(c, now))
            .count();
        if open >= limits.open_per_participant {
            return Evaluation::Refuse(Refusal::TooManyQuestions);
        }
        Evaluation::AskNew(QuestionInput {
            digest,
            has_input: arguments.has_input(),
            asked_of: invitation.by.clone(),
            turn,
            sensitive,
        })
    }

    /// Count a new question against this participant's hour.
    pub(crate) fn count_question(&mut self, caller: &str, now: DateTime<Utc>) {
        if let Some(invitation) = self.invited.get_mut(caller) {
            let hour_on = invitation
                .asked_since
                .is_some_and(|since| now < since + Duration::hours(1));
            if !hour_on {
                invitation.asked_since = Some(now);
                invitation.asked = 0;
            }
            invitation.asked += 1;
        }
    }

    /// Settle what has expired. Answers the ids.
    pub(crate) fn sweep_expired(&mut self, now: DateTime<Utc>) -> Vec<String> {
        let mut gone = vec![];
        for call in &mut self.calls {
            if matches!(call.stage, CallStage::Question | CallStage::Approved)
                && call.expires_at <= now
            {
                call.stage = CallStage::Cancelled;
                call.revision += 1;
                gone.push(call.id.clone());
            }
        }
        gone
    }

    /// New arguments for a call. Answers `true` if anything changed. Its expiry stays: new input must
    /// not keep a card alive for ever.
    pub(crate) fn apply_revise(
        &mut self,
        at: usize,
        arguments: &Arguments,
    ) -> Result<bool, Refusal> {
        let call = self.calls.get(at).ok_or(Refusal::UnknownCall)?;
        if call.stage.is_settled() {
            return Err(Refusal::SettledCall);
        }
        if call.stage == CallStage::Running {
            return Err(Refusal::RunningCall);
        }
        // The same rules as `check`, with the call as it stands: a revise can refuse where `check`
        // would, and it never allows. It names no object, so a read counts as marked.
        let marked = counts_as_marked(&call.capability, true);
        self.head(&call.caller, &call.capability)?;
        let standing = self.standing(
            &call.caller,
            &call.capability,
            arguments.has_input(),
            marked,
        );
        if let Standing::Refused(refusal) = standing {
            return Err(refusal);
        }
        let call = &mut self.calls[at];
        let digest = hex(&arguments_digest(
            &call.caller,
            call.turn,
            &call.capability,
            arguments,
        ));
        if digest == call.digest {
            return Ok(false);
        }
        if call.revision >= MAX_REVISION {
            return Err(Refusal::RetriesExhausted);
        }
        call.digest = digest;
        call.has_input = arguments.has_input();
        call.sensitive |= marked;
        call.stage = CallStage::Question;
        call.revision += 1;
        Ok(true)
    }
}

fn is_waiting(call: &CallRecord, now: DateTime<Utc>) -> bool {
    matches!(call.stage, CallStage::Question | CallStage::Approved) && call.expires_at > now
}
