//! Fetching: the [`MailFetch`] strategy, and the [`MailCursor`] that makes a restart resume.
//!
//! Pattern: Strategy (JMAP, IMAP) behind one small trait. A strategy only delivers bytes, untouched;
//! everything that reads them sits above it, so both behave the same.

mod cursor;
mod imap;

use async_trait::async_trait;

use crate::error::MailError;

pub use cursor::{MailCursor, MailboxCursor};
pub use imap::{ImapConfig, ImapFetch};

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

/// How mail is read from a server, and how a sent copy gets there.
#[async_trait]
pub trait MailFetch: Send + Sync {
    /// Up to `limit` mails after `cursor`, per mailbox. May repeat a mail; never skips one.
    async fn fetch(&self, cursor: &MailCursor, limit: usize) -> Result<FetchedBatch, MailError>;
    /// Keep `raw` as the sent copy.
    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError>;
}
