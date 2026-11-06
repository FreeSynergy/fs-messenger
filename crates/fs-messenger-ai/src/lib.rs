//! `fs-messenger-ai`: the AI model provider of FreeSynergy.Messenger.
//!
//! **Not checked against a real server.** llama.cpp stayed off and no API key exists. The wire formats
//! come from documentation read on 2026-11-06, listed with its sources in `DESIGN.md`, and from a
//! scripted local HTTP server in the tests.
//!
//! [`ModelProvider`] implements the core's `Provider` trait for two wire formats, llama.cpp's
//! `/v1/chat/completions` and Anthropic's `/v1/messages`. The core refuses every honest plan of a model
//! today (ADR 0018), so as things stand no text reaches a model. `DESIGN.md` says why, and what the owner
//! has to decide. Nothing here runs a tool. There is no MCP and no ACP.
//!
//! Pattern: Facade: this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// As in fs-messenger-core: every fallible fn returns a crate error, documented once on the error.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions
)]

mod config;
mod endpoint;
mod error;
mod limits;
mod provider;
mod redact;
mod secret;
mod tls;
mod turn;
mod wire;

pub use config::{ModelConfig, ToolOffer};
pub use endpoint::{DataPath, Endpoint};
pub use error::AiError;
pub use limits::Limits;
pub use provider::{Host, ModelProvider, ProviderLink, RemoteChoice};
pub use secret::ApiKey;
pub use tls::TlsTrust;
pub use turn::{LiveTurn, TurnFailure};
