//! `fs-messenger-mail`: the mail provider of FreeSynergy.Messenger.
//!
//! [`MailProvider`] implements the core's `Provider` trait. Behind it, fetching (JMAP, IMAP) and
//! submitting (SMTP, JMAP `EmailSubmission`) are two small strategies, [`fetch::MailFetch`] and
//! [`submit::MailSubmit`]. OpenPGP is rPGP, PGP/MIME per RFC 3156 with Header Protection per RFC 9788,
//! Autocrypt is our own (ADR 0018). Keys sit behind [`keys::KeyStore`]. See `DESIGN.md` of this crate.
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

mod address;
pub mod autocrypt;
mod error;
pub mod fetch;
pub mod jmap;
pub mod keys;
pub mod mime;
pub mod openpgp;
mod provider;
mod secret;
pub mod submit;
mod tls;

pub use address::normalise_address;
pub use error::MailError;
pub use provider::{MailIdentity, MailProvider};
pub use secret::Password;
pub use tls::{Endpoint, TlsTrust};
