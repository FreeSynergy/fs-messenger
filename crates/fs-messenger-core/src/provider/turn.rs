//! [`TurnScope`]: which running agent turns a provider is asked to end.
//!
//! Pattern: Value object: what the guard hands a provider for `stop_turns`. It carries only what the
//! provider issued itself, and gives no way back into the engine.

use crate::model::ExternalId;

/// Which turns to end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnScope {
    /// All of them: the emergency stop.
    All,
    /// Those of one conversation, by what the provider calls it.
    Conversation(Vec<ExternalId>),
}
