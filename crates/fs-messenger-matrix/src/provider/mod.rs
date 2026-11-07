//! [`MatrixProvider`]: one Matrix account behind the core's `Provider` trait.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules).

mod account;
mod connect;
mod convert;
mod deadline;
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
