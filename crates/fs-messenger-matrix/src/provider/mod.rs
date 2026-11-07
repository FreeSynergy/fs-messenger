//! [`MatrixProvider`]: one Matrix account behind the core's `Provider` trait.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules).

mod account;
#[allow(dead_code)] // a shell until the implementation
mod answer;
#[allow(dead_code)] // a shell until the implementation
mod check;
mod connect;
mod convert;
mod deadline;
mod echo;
mod fetch;
mod history;
mod matrix_provider;
mod plan;
#[allow(dead_code)] // a shell until the implementation
mod raw_sync;
mod replay;
mod rooms;
mod send;

pub use account::MatrixAccount;
#[allow(unused_imports)] // a shell until the implementation
pub(crate) use answer::{Answer, RoomAnswer};
pub use matrix_provider::MatrixProvider;
pub use rooms::Invite;
