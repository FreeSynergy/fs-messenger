//! [`Gate`] and [`Guard`]: the two faces of the engine.
//!
//! Pattern: Strategy (traits `Gate` and `Guard`): a tool layer holds a `dyn Gate` and has no way to a
//! `Guard` method. `Guard` is the contract's interface, for a person's device only, and every change
//! needs a device signature of a person. Neither is reachable from `Messenger` or `Provider`.

use async_trait::async_trait;
use fs_interfaces::provider::Unsupported;

use crate::arguments::Arguments;
use crate::error::{GuardError, Refusal};
use crate::model::{AiGrant, Caller, GuardProof, GuardState, StopReport, Verdict};
use crate::sensitivity::ObjectRef;
use fs_messenger_core::ToolCallOutcome;

/// Entry for anyone who wants to act. There is no way to hand it a risk.
#[async_trait]
pub trait Gate: Send + Sync {
    /// May this caller use this capability with these arguments?
    async fn check(&self, caller: &Caller, capability: &str, arguments: &Arguments) -> Verdict;

    /// As `check`, for a read that names the object it reads. The engine looks the object up in its
    /// `SensitivityLookup`; what is marked asks every time and cannot be released. What the arguments
    /// say about the object is never read.
    async fn check_object(
        &self,
        caller: &Caller,
        capability: &str,
        arguments: &Arguments,
        object: &ObjectRef,
    ) -> Verdict;

    /// New arguments for an open call the engine issued to this caller: its digest changes, its
    /// revision rises, and an `allowOnce` that was not used lapses. Same arguments change nothing.
    async fn revise(&self, caller: &Caller, call: &str, arguments: &Arguments) -> Verdict;

    /// A running call of this caller has ended: `done` or `failed`. Anything else is refused.
    async fn finished(
        &self,
        caller: &Caller,
        call: &str,
        outcome: ToolCallOutcome,
    ) -> Result<(), Refusal>;

    /// May this participant act as an invited AI? Needs the invitation and an open program. Not used
    /// by the messenger's way in: a message is stored for any sender (`SenderLookup` only marks it).
    async fn admit_message(&self, sender: &str) -> Result<(), Refusal>;
}

/// `Guard` of the contract: a person's device only; never generated into MCP.
#[async_trait]
pub trait Guard: Send + Sync {
    /// What is in force. Unreadable, missing or corrupt state reads as stopped, and as nothing else.
    async fn state(&self) -> GuardState;
    /// The person creates the AI's profile: that is the invitation. It may do nothing yet.
    async fn invite(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError>;
    /// What it may do. Refuses `unknown`, `destroys` and `actsForPerson`.
    async fn release(&self, grant: &AiGrant, proof: &GuardProof) -> Result<(), GuardError>;
    /// Takes all releases of this participant, in every program, and its open calls.
    async fn revoke(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError>;
    /// Takes the releases of this participant in one program, and its open calls of that program.
    async fn revoke_program(
        &self,
        profile: &str,
        program: &str,
        proof: &GuardProof,
    ) -> Result<(), GuardError>;
    /// Withdraws the invitation: its releases and open calls go, and it may ask no more. By who invited
    /// it, or the owner.
    async fn uninvite(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError>;
    /// Emergency stop. Stops in memory first, and reports what it could not do.
    async fn stop(&self, proof: &GuardProof) -> Result<StopReport, GuardError>;
    /// Opens it again. Only with the permission, on a person's own device. Releases stay taken.
    async fn reopen(&self, proof: &GuardProof) -> Result<(), GuardError>;
    /// A stop for one group. Groups come later (ADR 0054 point 5: a permission setting).
    async fn stop_group(
        &self,
        _group: &str,
        _proof: &GuardProof,
    ) -> Result<StopReport, GuardError> {
        Err(Unsupported::new("guard.stop.group", "groups are not built here yet").into())
    }
}
