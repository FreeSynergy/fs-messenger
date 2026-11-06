//! `MessageAction.decide` as a pure function.
//!
//! Pattern: State + Specification: every rule refuses without any change, and only the engine's own
//! record counts: not what a message shows, which an undo could bring back. The proof comes last, so a
//! wrong click uses up nothing; it covers the message, the call, the option, the digest and the revision
//! the person saw. A decision has no nonce; its signed bytes are single use.

use chrono::{DateTime, Utc};
use fs_messenger_core::{ToolCallChoice, ToolCallDecision};
use fs_store::ObjectId;

use super::calls::Limits;
use super::doc::{CallRecord, GuardDoc};
use super::stage::{CallStage, options_for};
use crate::device::DeviceVerifier;
use crate::digest::{hex, single_use_key, unhex};
use crate::error::Refusal;
use crate::model::{AiGrant, GuardMode};
use crate::risk::can_be_released;
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
    pub(crate) choice: ToolCallChoice,
    pub(crate) granted: Option<AiGrant>,
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
        if d.signer != call.asked_of {
            return Err(Refusal::SignerNotAskedOf);
        }
        if call.expires_at <= input.now {
            return Err(Refusal::Expired);
        }
        let risk = call.risk();
        let choice = options_for(risk, call.has_input)
            .into_iter()
            .find(|o| o.id == d.option)
            .ok_or(Refusal::UnknownOption)?
            .choice;
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
        let mut granted = None;
        match choice {
            ToolCallChoice::Refuse => call.stage = CallStage::Refused,
            ToolCallChoice::AllowOnce => {
                call.stage = CallStage::Approved;
                call.expires_at = input.now + input.limits.approval_ttl;
            }
            ToolCallChoice::AllowClass => {
                call.stage = CallStage::Approved;
                call.expires_at = input.now + input.limits.approval_ttl;
                if can_be_released(risk) {
                    granted = Some(AiGrant {
                        profile: call.caller.clone(),
                        risk,
                    });
                }
            }
        }
        let decided = Decided {
            call: call.clone(),
            choice,
            granted: granted.clone(),
        };
        self.used_proofs.insert(used);
        if let Some(grant) = granted {
            self.apply_grant(grant);
        }
        self.prune_settled();
        Ok(decided)
    }
}
