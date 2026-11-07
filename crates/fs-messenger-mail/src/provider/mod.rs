//! [`MailProvider`]: the core's `Provider` for mail.

mod contained;
mod facade;
mod ids;
mod opened;
mod reach;
mod read;
mod receive;
mod send;

pub use facade::{MailIdentity, MailProvider};
