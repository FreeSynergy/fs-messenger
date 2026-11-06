//! `fs-messenger-matrix`: the Matrix provider of FreeSynergy.Messenger.
//!
//! [`MatrixProvider`] implements the core's `Provider` trait over `matrix-sdk`. It sends only into
//! encrypted rooms and marks an incoming message `megolm` only if decryption really happened. The SDK's
//! stores live in an encrypted `fs-store`, in pure Rust, not in SQLite. See `DESIGN.md` of this crate.
//!
//! Pattern: Facade: this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// As in fs-messenger-core: every fallible fn returns the crate `Result`, documented once on the error.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions
)]

mod error;
mod provider;
mod receive;
mod secret;
pub mod store;

pub use error::{MatrixError, StoreFail};
pub use provider::{Invite, MatrixAccount, MatrixProvider};
pub use receive::{SCHEME_EVENT, SCHEME_ROOM, UNDECRYPTED_MARK};
pub use secret::Password;
