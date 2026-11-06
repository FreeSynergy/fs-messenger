//! What goes into the activity log, and how it reads back.
//!
//! Pattern: Adapter: one small event to a log-only activity. Log-only activities cannot be undone (the
//! store's rule), and they sit on an id of their own, which holds no object. No secrets: no proof, no
//! nonce, no key, no arguments, no digest. Refused attempts are not logged: a refusal changes nothing,
//! and a caller could otherwise fill the log. Use of a class release is logged at most once a minute
//! per participant and tool, for the same reason.

use chrono::{DateTime, Utc};
use fs_messenger_core::ToolCallRisk;
use fs_store::{ActivityKind, ObjectId};
use serde::{Deserialize, Serialize};

/// The fixed id the log entries hang on.
pub(crate) const LOG_SUBJECT: &str = "01900000-0000-7000-8000-00000000a1d1";

/// One entry, as it reads back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GuardLogEntry {
    /// `invite`, `uninvite`, `release`, `revoke`, `stop`, `reopen`, `decide`, `approvalUsed`, `classUsed`,
    /// `finished`, `enrol`, `removeDevice`, `recover`, `turnStop`.
    pub event: String,
    /// Who did it: the person who signed, or the engine for a stop without a proof.
    pub actor: String,
    /// When; set by the store, empty on writing.
    pub at: Option<DateTime<Utc>>,
    /// The participant or the person it is about.
    pub profile: String,
    /// The risk class it is about.
    pub risk: Option<ToolCallRisk>,
    /// The engine's id of the call.
    pub call: String,
    /// The tool's name.
    pub capability: String,
    /// `refuse`, `allowOnce`, `allowClass`.
    pub choice: String,
    /// How a call ended: `done`, `failed`.
    pub outcome: String,
    /// How many calls it cancelled.
    pub cancelled: usize,
}

impl GuardLogEntry {
    pub(crate) fn new(event: &str, actor: &str) -> Self {
        Self {
            event: event.to_string(),
            actor: actor.to_string(),
            ..Self::default()
        }
    }

    pub(crate) fn about(mut self, profile: &str) -> Self {
        self.profile = profile.to_string();
        self
    }

    /// The nearest AS2 kind; `event` says what it really was.
    pub(crate) fn kind(&self) -> ActivityKind {
        match self.event.as_str() {
            "invite" => ActivityKind::Invite,
            "release" | "enrol" => ActivityKind::Add,
            "revoke" | "uninvite" | "removeDevice" => ActivityKind::Remove,
            "reopen" | "recover" | "approvalUsed" | "classUsed" | "finished" => {
                ActivityKind::Accept
            }
            "decide" if self.choice != "refuse" => ActivityKind::Accept,
            _ => ActivityKind::Reject,
        }
    }
}

pub(crate) fn subject() -> ObjectId {
    LOG_SUBJECT.parse().expect("a fixed, valid id")
}
