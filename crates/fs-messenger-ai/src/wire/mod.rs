//! The wire: server-sent events, the two dialects, one HTTP way to reach them.

mod anthropic;
mod dialect;
mod http;
mod llama;
mod sse;

pub(crate) use anthropic::Anthropic;
pub(crate) use dialect::{ChatLine, Dialect, Ending, ModelEvent, Role, TurnUsage};
pub(crate) use http::{Control, Transport};
pub(crate) use llama::LlamaCpp;
pub(crate) use sse::SseReader;
