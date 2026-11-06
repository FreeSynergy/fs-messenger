//! `fs-messenger-core`: program core of FreeSynergy.Messenger (no GUI, no RPC, no real connectors).
//!
//! One message model over `fs-store` for mail, chat and agents (ADR 0018, 0054). Start with the
//! [`Messenger`] trait; [`MessengerRepo`] is its store-backed implementation. Connectors plug in as
//! [`Provider`]s. Encrypted only is enforced here, not in the providers. See `DESIGN.md`.
//!
//! Pattern: Facade: this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// Pedantic lints that would only add noise: every fallible fn returns the crate `Result`
// (documented once on `Error`), and results of pure getters are used by callers anyway.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions
)]

mod draft;
mod error;
mod gate;
mod guard_hook;
mod ingest;
mod ingest_rules;
mod listener;
mod messenger;
pub mod model;
pub mod provider;
mod repo;

pub use draft::{Draft, DraftInfo};
pub use error::{Error, Result};
pub use guard_hook::{EngineGuard, SenderLookup};
pub use ingest::{Ingest, Ingested, SyncReport};
pub use listener::{Listener, Subscription};
pub use messenger::{CONTRACT_VERSION, Messenger};
pub use model::*;
pub use provider::{
    FetchBatch, Outgoing, OutgoingAttachment, Provider, ProviderCursor, RawAttachment, RawMessage,
    RawToolCall, SendReceipt, TurnScope,
};
pub use repo::{
    GuardPort, MessengerConfig, MessengerRepo, PlacedCall, SettledCalls, TurnStop, TurnStopOutcome,
};
