//! `fs-messenger-app`: the window of the messenger (ADR 0065, 0066).
//!
//! The window shell and the generated Admin frontend come from `fs-components`; this crate gives them the type
//! descriptions of `fs-messenger-core` and a [`fs_admin_frontend::Backend`] per data type over the core. No drawing code
//! and no `blinc_*` import (ADR 0066): the look is not this crate's.
//!
//! Pattern: Facade — this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

mod accounts;
mod app;
mod args;
mod conversations;
mod convert;
mod drafts;
mod messages;
mod shared;
mod store;
mod window;
mod words;

pub use accounts::AccountBackend;
pub use app::app_command;
pub use args::{Args, HELP, parse};
pub use conversations::ConversationBackend;
pub use drafts::DraftBackend;
pub use messages::MessageBackend;
pub use shared::Shared;
pub use store::{KeyProblem, open_repo};
pub use window::{about, build_window};
pub use words::{Words, frontend_keys};
