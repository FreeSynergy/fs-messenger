//! Fetching: the [`MailFetch`] strategy, and the [`MailCursor`] that makes a restart resume.
//!
//! Pattern: Strategy (JMAP, IMAP) behind one small trait. A strategy only delivers bytes, untouched;
//! everything that reads them sits above it, so both behave the same.

mod cursor;
mod imap;

use async_trait::async_trait;

use crate::error::MailError;

pub use cursor::{MailCursor, MailboxCursor};
pub use imap::{DEFAULT_IMAP_TIMEOUT, ImapConfig, ImapFetch};

/// What a mailbox is for. Only `Sent` can make a mail "mine": the `From` of a mail proves nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxRole {
    /// Incoming.
    Inbox,
    /// Sent copies.
    Sent,
}

/// One mail, as the server has it.
#[derive(Clone, Debug)]
pub struct FetchedMail {
    /// The bytes, as they are on the server (ADR 0030).
    pub bytes: Vec<u8>,
    /// Which mailbox it is from.
    pub role: MailboxRole,
    /// Stable within the account: mailbox and the server's own id. For a mail without `Message-ID`.
    pub stable_id: String,
}

/// One answer of a strategy.
#[derive(Clone, Debug, Default)]
pub struct FetchedBatch {
    /// The mails.
    pub mails: Vec<FetchedMail>,
    /// Where to go on from.
    pub cursor: MailCursor,
    /// Whether another call would deliver more right now.
    pub more: bool,
}

/// How much one call may deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchLimits {
    /// Mails per mailbox.
    pub mails: usize,
    /// Bytes per mail. A larger one is skipped, logged and left on the server: holding it just to
    /// have the core refuse it would let a sender fill the memory.
    pub bytes: usize,
}

/// A failure in the sent mailbox does not cost what the inbox delivered: the cursor of the sent mailbox
/// stays where it was, and the next call tries again.
pub(crate) fn sent_is_optional(result: Result<(), MailError>) {
    if let Err(e) = result {
        tracing::warn!(error = %e, "could not read the sent mailbox; going on with the inbox");
    }
}

/// How mail is read from a server, and how a sent copy gets there.
#[async_trait]
pub trait MailFetch: Send + Sync {
    /// What comes after `cursor`, within `limits`. May repeat a mail; skips only one over the limit.
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError>;
    /// Keep `raw` as the sent copy.
    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError>;
}
