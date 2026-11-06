//! [`Limits`]: what a server can make a turn cost at most.
//!
//! Pattern: Value object. The numbers are provisional: nothing here was measured against a real server.

use std::time::Duration;

/// The hard limits of a turn. All of them end it as `failed`, with the text kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Nothing moves for this long. Also the time to connect and to get the first answer. A model that
    /// takes minutes on its prompt needs more.
    pub idle: Duration,
    /// A whole turn, retries included, so a server that trickles for ever ends too.
    pub turn: Duration,
    /// Bytes on the wire. Below the core's 25 MiB per message, so an answer is never a stub.
    pub response_bytes: usize,
    /// One line, and one server-sent event.
    pub event_bytes: usize,
    /// Reasoning kept. The core asks a connector to cap it: it stays in the message, and each change
    /// logs it again.
    pub reasoning_bytes: usize,
    /// One tool call's arguments, as shown.
    pub tool_argument_bytes: usize,
    /// Tool calls shown per turn.
    pub tool_calls: usize,
    /// Retries after the first request, before any answer began. Never after.
    pub retries: u32,
    /// The longest `retry-after` obeyed. A longer one ends the turn at once, naming the wait.
    pub retry_after_cap: Duration,
    /// The first wait when a server names none; it doubles.
    pub backoff: Duration,
    /// The request body.
    pub request_bytes: usize,
    /// Earlier messages sent along.
    pub history_messages: usize,
    /// Bytes of earlier messages sent along.
    pub history_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(120),
            turn: Duration::from_secs(30 * 60),
            response_bytes: 16 * 1024 * 1024,
            event_bytes: 1024 * 1024,
            reasoning_bytes: 1024 * 1024,
            tool_argument_bytes: 64 * 1024,
            tool_calls: 64,
            retries: 2,
            retry_after_cap: Duration::from_secs(30),
            backoff: Duration::from_millis(250),
            request_bytes: 8 * 1024 * 1024,
            history_messages: 64,
            history_bytes: 1024 * 1024,
        }
    }
}
