//! Refusals and errors.
//!
//! Pattern: none (plain enums via `thiserror`). A [`Refusal`] means nothing changed, and names the rule.
//! A [`GuardError`] is about the rest: what is not built, and what the store did.

use fs_interfaces::provider::Unsupported;
use fs_messenger_core::ToolCallRisk;

/// Why something was refused. Nothing changed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The program is stopped, or its state cannot be read, which reads the same.
    #[error("the guard is stopped")]
    Stopped,
    /// No person invited this participant.
    #[error("this participant was not invited")]
    NotInvited,
    /// Only a person's device may do this; no AI can be asked for it.
    #[error("never for an AI")]
    NeverForAi,
    /// The name of a capability or profile is empty, too long or has control characters.
    #[error("not a usable name")]
    BadName,
    /// Too many open questions for this participant.
    #[error("too many open questions for this participant")]
    TooManyQuestions,
    /// Asked again too soon.
    #[error("asked again too soon")]
    RetryTooSoon,
    /// Asked again too often; the question stays open for the person.
    #[error("asked again too often")]
    RetriesExhausted,
    /// There is no messenger to put the question in, or it would not take it.
    #[error("no way to ask a person")]
    NoWayToAsk,
    /// The state could not be written, so nothing was allowed.
    #[error("the state could not be written")]
    NotRecorded,
    /// A required field is empty.
    #[error("empty: {0}")]
    Empty(&'static str),
    /// No such call, or not this caller's, or not at this place.
    #[error("no such call")]
    UnknownCall,
    /// The call is settled.
    #[error("the call is settled")]
    SettledCall,
    /// The call is running; its input cannot change any more.
    #[error("the call is running")]
    RunningCall,
    /// Not one of the options of this call.
    #[error("unknown option")]
    UnknownOption,
    /// Someone other than the person asked.
    #[error("the signer is not the person asked")]
    SignerNotAskedOf,
    /// The digest is not that of the call.
    #[error("the digest does not match the call")]
    DigestMismatch,
    /// The revision is not that of the call.
    #[error("the revision does not match the call")]
    RevisionMismatch,
    /// This proof was used before.
    #[error("this proof was used before")]
    Replayed,
    /// The question has expired.
    #[error("the question has expired")]
    Expired,
    /// The signature does not fit, or the signer has no such key.
    #[error("the device proof does not hold")]
    BadProof,
    /// The signer is marked as AI. An agent is never an admin (ADR 0056).
    #[error("the signer is marked as AI")]
    SignerIsAi,
    /// The signer lacks the permission.
    #[error("the signer may not do this")]
    NotPermitted,
    /// This class can never be released.
    #[error("{0:?} can never be released")]
    NotReleasable(ToolCallRisk),
    /// Already invited.
    #[error("already invited")]
    AlreadyInvited,
    /// A person cannot be invited as an AI, and an AI cannot have a device.
    #[error("this identity is a person with a device, or an AI")]
    IdentityConflict,
    /// It is not stopped.
    #[error("it is open already")]
    AlreadyOpen,
    /// The state is damaged; only the local recovery path can go on.
    #[error("the state is damaged")]
    Damaged,
    /// Not a usable device key.
    #[error("not a usable device key")]
    BadKey,
    /// This key cannot be enrolled this way.
    #[error("not on this path: {0}")]
    NotEnrollable(&'static str),
}

/// What can go wrong apart from a refusal.
#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    /// Refused; nothing changed.
    #[error(transparent)]
    Refused(#[from] Refusal),
    /// The explicit "not possible" (ADR 0045).
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
    /// The state could not be written. Nothing changed, unless it was a stop: see `StopReport`.
    #[error("the state could not be written: {0}")]
    NotPersisted(String),
    /// It happened, and the state holds it; but the log, or what is shown, could not be written.
    #[error("done, but not logged or shown: {0}")]
    DoneNotShown(String),
}

impl From<GuardError> for fs_messenger_core::Error {
    fn from(e: GuardError) -> Self {
        match e {
            GuardError::Unsupported(u) => Self::Unsupported(u),
            other => Self::GuardRefused(other.to_string()),
        }
    }
}
