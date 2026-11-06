//! [`MessageAction`]: what a person can do to one message.
//!
//! Pattern: Command: one enum of operations, applied by `Messenger::act`. Deliberately no `Default`:
//! an action is never assumed. `None` mirrors ordinal 0 of the contract and is refused.

use super::ToolCallDecision;

/// An action on one message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageAction {
    /// Unset on the wire: does nothing and is refused with an error.
    None,
    /// Delete it; a tombstone stays, so a replay does not bring it back.
    Delete,
    /// Set or clear the flag.
    Flag(bool),
    /// Mark it unread or read.
    Unread(bool),
    /// React with this key; empty takes my reaction back. Not possible yet: nothing carries it.
    React(String),
    /// Send it on to these recipients; the same encryption rule applies as for any send.
    Forward(Vec<String>),
    /// New text of my own message. Not possible yet: nothing carries it.
    Edit(String),
    /// Answers a question of an agent. Not possible yet: the guard follows.
    Decide(ToolCallDecision),
}
