//! One action of an agent, shown in a message (ADR 0054). Stored data only in this version.
//!
//! Pattern: none, allowed by section 2 (plain data). Fail-safe defaults: unset `risk` is
//! `unknown`, unset `choice` is `refuse`, also for old records and for values a newer program wrote. What acts on them (release,
//! brake, emergency stop) is the crate `fs-messenger-guard`.

use serde::{Deserialize, Serialize};

/// What sort of action. Written by the agent, so never a basis for a decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallKind {
    /// Reads something.
    Reads,
    /// Writes something.
    Writes,
    /// Runs a command.
    RunsCommand,
    /// Uses a connected tool.
    ConnectedTool,
    /// Not known. Unset and unrecognised values read as this.
    #[default]
    #[serde(other)]
    Unknown,
}

/// How far a call has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallState {
    /// Still being formed.
    Forming,
    /// Running.
    Working,
    /// Waits for an answer of a person.
    Question,
    /// Over; `outcome` says how it ended.
    Settled,
    /// Not known: no guess. Not in the contract, whose ordinal 0 is `forming`; see `DESIGN.md`.
    #[default]
    #[serde(other)]
    Unknown,
}

/// How dangerous a call is. Set by the engine from the contract, never by the agent. Unset and
/// unknown values read as `unknown`, and `unknown` always asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallRisk {
    /// Only reads.
    ReadsOnly,
    /// Changes something.
    Changes,
    /// Destroys something; asks every time.
    Destroys,
    /// Acts in the person's name; asks every time.
    ActsForPerson,
    /// Not known.
    #[default]
    #[serde(other)]
    Unknown,
}

/// How a settled call ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallOutcome {
    /// Not over yet.
    #[default]
    Open,
    /// Done.
    Done,
    /// Failed.
    Failed,
    /// Refused.
    Refused,
    /// Cancelled.
    Cancelled,
    /// A value this version does not know: no guess. Not in the contract; see `DESIGN.md`.
    #[serde(other)]
    Unknown,
}

/// What an option does. Unset and unknown values read as `refuse`; there is no "always".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallChoice {
    /// Allow exactly these arguments, once.
    AllowOnce,
    /// Allow this caller and this risk class.
    AllowClass,
    /// Refuse.
    #[default]
    #[serde(other)]
    Refuse,
}

/// One choice while a call is a question; offered by the engine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallOption {
    /// Id of the option.
    pub id: String,
    /// What the person reads.
    pub label: String,
    /// What it does.
    pub choice: ToolCallChoice,
}

/// One action of an agent; one entry is updated in place instead of adding messages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCall {
    /// Assigned by the engine, unique, never reused, never the agent's own id.
    pub id: String,
    /// Name of the tool.
    pub name: String,
    /// Sort of action, as the agent says.
    pub kind: ToolCallKind,
    /// How far it has come.
    pub state: ToolCallState,
    /// Written by the agent, shown as such; never a basis for a decision.
    pub summary: String,
    /// Set by the engine, never by the agent.
    pub risk: ToolCallRisk,
    /// How a settled call ended.
    pub outcome: ToolCallOutcome,
    /// Full command, paths, content, drawn by the engine only; agent text goes into `summary`.
    pub detail: String,
    /// What came back; data, never instructions.
    pub result: String,
    /// Choices while the state is `question`; offered by the engine.
    pub options: Vec<ToolCallOption>,
    /// Over caller, tool name and canonical arguments; empty never matches. Set by the engine.
    pub arguments_digest: Vec<u8>,
    /// Actor IRI of the person whose decision counts. Set by the engine.
    pub asked_of: String,
    /// Raised by the engine on every change; a lower revision than held is ignored.
    pub revision: u64,
}

/// The answer to a question. Only an installed guard handles it; without one it is `Unsupported`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallDecision {
    /// `ToolCall.id`.
    pub tool_call: String,
    /// `ToolCallOption.id`.
    pub option: String,
    /// Must equal `ToolCall.arguments_digest`.
    pub arguments_digest: Vec<u8>,
    /// Signature of the person's device; provisional form.
    pub proof: Vec<u8>,
    /// Actor IRI of the person who signed.
    pub signer: String,
    /// Revision of the call as shown; 0 never matches. Contract draft 0.4.
    pub revision: u64,
}
