//! [`Dialect`] and [`StreamReader`]: what differs between the two wire formats.
//!
//! Pattern: Strategy: one flow in `turn/`, two dialects. Both turn a server's stream into the same small
//! [`ModelEvent`]s, which claim nothing: no risk, no options, no decision, no sender. A dialect holds no
//! connection and no secret beyond the header it writes.

use reqwest::header::{HeaderName, HeaderValue};

use super::sse::SseEvent;
use crate::config::ModelConfig;
use crate::error::AiError;
use crate::secret::ApiKey;
use crate::turn::TurnFailure;

/// Who said an earlier line. There is no system role here: only the program's own `system` setting has
/// one, and it never holds a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
}

impl Role {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// One message of the conversation so far. Its text is data, whatever it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatLine {
    pub(crate) role: Role,
    pub(crate) text: String,
}

/// What a server reported about a turn. Zero is not known. Never a cost: neither API states one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) context_used: u64,
}

/// How a stream said it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The answer is what the model meant to say.
    Complete,
    /// The server stopped it for this reason: cut off, refused, or not known here.
    Cut(String),
}

/// One thing a stream said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ModelEvent {
    Text(String),
    Reasoning(String),
    ToolStart {
        index: u64,
        agent_ref: String,
        name: String,
    },
    ToolArguments {
        index: u64,
        part: String,
    },
    ToolEnd {
        index: u64,
    },
    Usage(TurnUsage),
    End(Ending),
}

/// The state of one stream.
pub(crate) trait StreamReader: Send {
    /// What one server-sent event says. An error ends the turn.
    fn event(&mut self, event: &SseEvent) -> Result<Vec<ModelEvent>, TurnFailure>;
    /// The body ended and no event ended the stream.
    fn end_of_body(&mut self) -> Result<Vec<ModelEvent>, TurnFailure>;
}

/// One wire format.
pub(crate) trait Dialect: Send + Sync {
    /// The path of the streaming request.
    fn path(&self) -> &'static str;
    /// The headers, without the content type. The credential is marked sensitive.
    fn headers(&self, key: Option<&ApiKey>) -> Result<Vec<(HeaderName, HeaderValue)>, AiError>;
    /// The request body.
    fn body(&self, config: &ModelConfig, lines: &[ChatLine]) -> serde_json::Value;
    /// A reader for one new stream.
    fn reader(&self) -> Box<dyn StreamReader>;
}

/// `Authorization: Bearer`, which both APIs document.
pub(crate) fn bearer(key: &ApiKey) -> Result<(HeaderName, HeaderValue), AiError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {}", key.expose()))
        .map_err(|_| AiError::Config("the API key cannot go into a header".into()))?;
    value.set_sensitive(true);
    Ok((reqwest::header::AUTHORIZATION, value))
}

/// A server's word for something, as a short safe name, in the form both APIs document (`rate_limit_error`,
/// `end_turn`): text from a server is not shown otherwise, so it cannot carry anything into a message.
pub(crate) fn short_name(word: Option<&str>) -> String {
    match word {
        Some(w)
            if !w.is_empty()
                && w.len() <= 64
                && w.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') =>
        {
            w.to_string()
        }
        _ => "unknown".to_string(),
    }
}
