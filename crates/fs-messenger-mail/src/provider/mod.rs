//! [`MailProvider`]: the core's `Provider` for mail.

mod facade;
mod inspect;
mod receive;
mod send;

pub use facade::{MailIdentity, MailProvider};
pub use inspect::Inspection;
pub use receive::UNDECRYPTED_MARK;
