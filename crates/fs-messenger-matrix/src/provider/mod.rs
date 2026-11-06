//! [`MatrixProvider`]: one Matrix account behind the core's `Provider` trait.
//!
//! Pattern: Facade: this file only declares modules and re-exports.

mod account;
mod connect;
mod convert;
mod echo;
mod facade;
mod fetch;
mod history;
mod plan;
mod rooms;
mod send;

pub use account::MatrixAccount;
pub use facade::MatrixProvider;
pub use rooms::Invite;
