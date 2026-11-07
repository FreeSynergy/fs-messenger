//! The contract's guard types, and what `check` answers. Names mirror `messenger.capnp` draft 0.4.
//!
//! Pattern: none, allowed by section 2 (plain data; the two `Caller` constructors only fill fields). `GuardMode` reads as `Stopped` when unset or unrecognised, as the contract
//! says. `Caller` and `Verdict` are not in the contract: they are the engine's inner entry.

use std::time::Duration;

use chrono::{DateTime, Utc};
use fs_messenger_core::{ToolCall, ToolCallRisk, TurnStop};
use fs_objects::ObjectId;
use serde::{Deserialize, Serialize};

use crate::error::Refusal;

/// Stopped or open. Unset and anything unrecognised reads as stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuardMode {
    /// Open: invited participants go through release and brake.
    Open,
    /// Every AI participant is refused.
    #[default]
    #[serde(other)]
    Stopped,
}

/// `GuardState` of the contract.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardState {
    /// Stopped or open.
    pub mode: GuardMode,
    /// What is released.
    pub grants: Vec<AiGrant>,
    /// Who is invited.
    pub invited: Vec<String>,
    /// Who pulled the stop; empty if nobody did or it cannot be read.
    pub stopped_by: String,
    /// When.
    pub stopped_at: Option<DateTime<Utc>>,
}

/// `AiGrant` of the contract: one participant, one risk class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiGrant {
    /// The participant.
    pub profile: String,
    /// The class.
    pub risk: ToolCallRisk,
}

/// `GuardProof` of the contract.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardProof {
    /// Actor IRI of the person.
    pub signer: String,
    /// Single use.
    pub nonce: Vec<u8>,
    /// Device signature over [`signed_bytes`](crate::signed_bytes).
    pub proof: Vec<u8>,
}

/// Who wants to act. Built by the engine's own tool layer from how the caller reached it (for an agent,
/// its system user), never from what the caller says. Provisional until that layer exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// The participant.
    pub profile: String,
    /// The conversation of its running turn, if any. Not part of the digest.
    pub conversation: Option<ObjectId>,
}

impl Caller {
    /// A participant outside any conversation.
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            conversation: None,
        }
    }

    /// A participant in a running turn of this conversation.
    pub fn in_conversation(profile: impl Into<String>, conversation: ObjectId) -> Self {
        Self {
            profile: profile.into(),
            conversation: Some(conversation),
        }
    }
}

/// Why it is allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Allowance {
    /// A release of this class. Nothing is recorded per call.
    Class(ToolCallRisk),
    /// One `allowOnce` of exactly these arguments, used up now. Report the end with `Gate::finished`.
    Once {
        /// The engine's id of the call.
        call: String,
    },
}

/// A question that stands in the messenger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The call as shown.
    pub call: ToolCall,
    /// Its conversation.
    pub conversation: ObjectId,
    /// Its message.
    pub message: ObjectId,
    /// Do not ask again before this has passed.
    pub retry_after: Duration,
}

/// The answer of `check`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Go ahead. This is a snapshot: it cannot call back what has already started.
    Allow(Allowance),
    /// A person has to answer; the question stands in the messenger.
    Ask(Box<Question>),
    /// No.
    Refuse(Refusal),
}

/// What an emergency stop did. `check` refuses every AI whatever this says. Steps that are still running
/// go on after this was answered, and say so here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopReport {
    /// Why it could not be written. If it could not, a restart may forget this stop, until a later
    /// write succeeds; the engine tries again with every `check`.
    pub not_persisted: Option<String>,
    /// How many open calls were settled as `cancelled`, as far as the messenger could say.
    pub calls_cancelled: usize,
    /// Why the marker file beside the store could not be written. With `not_persisted` too, a restart
    /// forgets this stop: that is the limit `DESIGN.md` states.
    pub not_marked: Option<String>,
    /// Why what is shown could not be settled, or that a message could not be read.
    pub not_shown: Option<String>,
    /// Every provider's answer. Anything but `Ended` means its work may go on.
    pub turns: Vec<TurnStop>,
    /// Why the log has no entry.
    pub not_logged: Option<String>,
    /// Whether a messenger was attached at all.
    pub messenger_attached: bool,
}
