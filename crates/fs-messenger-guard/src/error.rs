//! Refusals and errors.
//!
//! Pattern: none, allowed by section 2 (error lists via `thiserror`; `decision_problem` and `From` only rename a variant). A [`Refusal`] means nothing changed, and names the rule.
//! A [`GuardError`] is about the rest: what is not built, and what the store did.

use fs_interfaces::provider::Unsupported;
use fs_messenger_core::{DecisionProblem, ToolCallRisk};

/// Why something was refused. Nothing changed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The program is stopped, or its state cannot be read, which reads the same.
    #[error("the guard is stopped")]
    Stopped,
    /// Nothing is released for this call: no release of its class in its program (or, for what always
    /// asks, none of any class in that program). It is not asked, so an AI cannot flood a person with
    /// questions: the person gives the release of their own accord.
    #[error("nothing is released for this")]
    NotReleased,
    /// No person invited this participant.
    #[error("this participant was not invited")]
    NotInvited,
    /// Only a person's device may do this; no AI can be asked for it.
    #[error("never for an AI")]
    NeverForAi,
    /// The name of a capability or profile is empty, too long or has control characters.
    #[error("not a usable name")]
    BadName,
    /// A person refused a question of this participant a moment ago: it may not ask right now.
    #[error("this participant was refused a moment ago and may not ask yet")]
    Paused,
    /// This participant has asked as often as it may in this hour.
    #[error("this participant has asked too often in this hour")]
    TooManyAsked,
    /// The guard cannot say now: the store does not answer, or its state is damaged.
    #[error("the guard cannot answer now")]
    Unavailable,
    /// The last key of the owner cannot go: nobody could ever open the program again.
    #[error("this is the owner's last key")]
    LastOwnerKey,
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

impl Refusal {
    /// The named problem of the contract, when this refuses a decision. Whatever has no name of its
    /// own (an id that is no call here, a stopped program with a call still open) is `Invalid`.
    pub fn decision_problem(&self) -> DecisionProblem {
        match self {
            Self::DigestMismatch | Self::RevisionMismatch => DecisionProblem::Stale,
            Self::SettledCall => DecisionProblem::Settled,
            Self::SignerNotAskedOf => DecisionProblem::NotAskedOf,
            _ => DecisionProblem::Invalid,
        }
    }
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
    /// This guard cannot be put into this messenger, which is fatal for the program: without it,
    /// `decide` and `stop` answer `Unsupported`.
    #[error("the guard could not be attached: {0}")]
    AttachRefused(String),
    /// It happened, and the state holds it; but the log, or what is shown, could not be written.
    #[error("done, but not logged or shown: {0}")]
    DoneNotShown(String),
}

impl From<GuardError> for fs_messenger_core::Error {
    fn from(e: GuardError) -> Self {
        match e {
            GuardError::Unsupported(u) => Self::Unsupported(u),
            GuardError::Refused(Refusal::Unavailable) => {
                Self::GuardUnavailable(Refusal::Unavailable.to_string())
            }
            other => Self::GuardRefused(other.to_string()),
        }
    }
}
