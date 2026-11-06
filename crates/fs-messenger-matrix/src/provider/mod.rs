//! [`MatrixProvider`]: one Matrix account behind the core's `Provider` trait.

mod account;
mod connect;
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
