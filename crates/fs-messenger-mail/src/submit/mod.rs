//! Submitting: the [`MailSubmit`] strategy.
//!
//! Pattern: Strategy (SMTP, JMAP `EmailSubmission`) behind one small trait. A strategy gets finished
//! bytes and a list of addresses; it never sees a plaintext, so it cannot leak one.

mod smtp;

use async_trait::async_trait;

use crate::error::MailError;

pub use smtp::{SmtpConfig, SmtpSubmit};

/// Who a mail goes to on the wire, hidden recipients included.
#[derive(Clone, Debug)]
pub struct MailEnvelope {
    /// `MAIL FROM`.
    pub from: String,
    /// `RCPT TO`, everyone.
    pub recipients: Vec<String>,
}

/// What a submission did beyond handing the mail over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SentCopy {
    /// The sent copy is on the server already.
    Stored,
    /// Nobody kept a copy; the caller has to.
    NotStored,
}

/// How a finished mail leaves.
#[async_trait]
pub trait MailSubmit: Send + Sync {
    /// Hand `raw` over. An error means it did not leave, as far as the transport can tell.
    async fn submit(&self, envelope: &MailEnvelope, raw: &[u8]) -> Result<SentCopy, MailError>;
}
