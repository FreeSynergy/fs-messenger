//! `MessageAction.decide` as a pure function.
//!
//! Pattern: State (pure transitions on `CallStage`): every rule refuses without any change, and only the engine's own
//! record counts: not what a message shows, which an undo could bring back. The proof comes last, so a
//! wrong click uses up nothing; it covers the message, the call, the option, the digest and the revision
//! the person saw. A decision has no nonce; its signed bytes are single use.

use chrono::{DateTime, Utc};
use fs_messenger_core::{ToolCallChoice, ToolCallDecision};
use fs_objects::ObjectId;

use super::calls::Limits;
use super::doc::{CallRecord, GuardDoc};
use super::stage::{Answer, CallStage, options_for};
use crate::device::DeviceVerifier;
use crate::digest::{hex, single_use_key, unhex};
use crate::error::Refusal;
use crate::identity::id_key;
use crate::model::GuardMode;
use crate::signed::{Method, decide_arguments, signed_bytes};

/// A decision, and where it was made.
pub(crate) struct DecideInput<'a> {
    pub(crate) conversation: ObjectId,
    pub(crate) message: ObjectId,
    pub(crate) decision: &'a ToolCallDecision,
    pub(crate) now: DateTime<Utc>,
    pub(crate) limits: &'a Limits,
}

/// What a decision did.
#[derive(Debug)]
pub(crate) struct Decided {
    pub(crate) call: CallRecord,
    pub(crate) answer: Answer,
}

fn refuse_empty(d: &ToolCallDecision) -> Result<(), Refusal> {
    let empty = [
        (d.tool_call.is_empty(), "toolCall"),
        (d.option.is_empty(), "option"),
        (d.arguments_digest.is_empty(), "argumentsDigest"),
        (d.proof.is_empty(), "proof"),
        (d.signer.is_empty(), "signer"),
    ];
    match empty.into_iter().find(|(is_empty, _)| *is_empty) {
        Some((_, name)) => Err(Refusal::Empty(name)),
        None => Ok(()),
    }
}

impl GuardDoc {
    /// Apply a decision to this state, or refuse. On a refusal the state must be thrown away by the
    /// caller if it wants to be sure; nothing here touches it before the last check.
    pub(crate) fn apply_decision(
        &mut self,
        input: &DecideInput<'_>,
        verifier: &dyn DeviceVerifier,
    ) -> Result<Decided, Refusal> {
        let d = input.decision;
        refuse_empty(d)?;
        let at = self
            .calls
            .iter()
            .position(|c| c.id == d.tool_call)
            .ok_or(Refusal::UnknownCall)?;
        let call = &self.calls[at];
        if call.conversation != input.conversation || call.message != input.message {
            return Err(Refusal::UnknownCall);
        }
        if call.stage.is_settled() {
            return Err(Refusal::SettledCall);
        }
        if self.mode != GuardMode::Open {
            return Err(Refusal::Stopped);
        }
        // Answered already and not over: what was shown has moved on.
        if call.stage != CallStage::Question {
            return Err(Refusal::RevisionMismatch);
        }
        if call.asked_of.is_empty() {
            return Err(Refusal::Empty("askedOf"));
        }
        if id_key(&d.signer) != call.asked_of {
            return Err(Refusal::SignerNotAskedOf);
        }
        if call.expires_at <= input.now {
            return Err(Refusal::Expired);
        }
        // Only what the question offers can be answered. `allowClass` is a word of the contract and
        // is on offer nowhere (strategy 4d(1)): an answer that names it is an unknown option.
        let answer = match options_for()
            .into_iter()
            .find(|o| o.id == d.option)
            .ok_or(Refusal::UnknownOption)?
            .choice
        {
            ToolCallChoice::Refuse => Answer::Refuse,
            ToolCallChoice::AllowOnce => Answer::Once,
            ToolCallChoice::AllowClass => return Err(Refusal::UnknownOption),
        };
        // Only an answer that lets the call go on needs a release, as seen now (open point 14).
        if answer == Answer::Once {
            self.approval_standing(call)?;
        }
        if hex(&d.arguments_digest) != call.digest {
            return Err(Refusal::DigestMismatch);
        }
        if d.revision == 0 || d.revision != call.revision {
            return Err(Refusal::RevisionMismatch);
        }
        let engine = unhex(&self.engine).ok_or(Refusal::Damaged)?;
        let message = input.message.to_string();
        let revision = d.revision.to_be_bytes();
        let arguments = decide_arguments(
            &message,
            &d.tool_call,
            &d.option,
            &d.arguments_digest,
            &revision,
        );
        let signed = signed_bytes(&engine, Method::Decide, &d.signer, &arguments, &[])
            .ok_or(Refusal::BadProof)?;
        self.signature_holds(verifier, &d.signer, &signed, &d.proof)?;
        let used = single_use_key(b"decide", &[&signed]);
        if self.used_proofs.contains(&used) {
            return Err(Refusal::Replayed);
        }

        // From here on it holds.
        let call = &mut self.calls[at];
        call.revision += 1;
        let mut paused = None;
        match answer {
            Answer::Refuse => {
                call.stage = CallStage::Refused;
                paused = Some(call.caller.clone());
            }
            Answer::Once => {
                call.stage = CallStage::Approved;
                call.expires_at = input.now + input.limits.approval_ttl;
            }
        }
        let decided = Decided {
            call: call.clone(),
            answer,
        };
        self.used_proofs.insert(used);
        if let Some(invitation) = paused.and_then(|caller: String| self.invited.get_mut(&caller)) {
            invitation.paused_until = Some(input.now + input.limits.refusal_pause);
        }
        Ok(decided)
    }
}
