//! `fs-messenger-matrix`: the Matrix provider of FreeSynergy.Messenger.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions
)]

mod error;
mod secret;
pub mod store;

pub use error::{MatrixError, StoreFail};
pub use secret::Password;
