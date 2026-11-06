//! [`MailProvider`]: the core's `Provider` for mail.

mod facade;
mod ids;
mod read;
mod receive;
mod send;

pub use facade::{MailIdentity, MailProvider};
