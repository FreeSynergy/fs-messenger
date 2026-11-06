//! Fetching: the [`MailFetch`] strategy, and the [`MailCursor`] that makes a restart resume.
//!
//! Pattern: Strategy (JMAP, IMAP) behind one small trait. A strategy only delivers bytes, untouched;
//! everything that reads them sits above it, so both behave the same.

mod cursor;
mod idle;
mod imap;
mod imap_mailbox;

use async_trait::async_trait;

use crate::error::MailError;

pub use cursor::{MailCursor, MailboxCursor};
pub use imap::{ImapConfig, ImapFetch};

/// How long a connection may stand still.
pub const DEFAULT_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Of a mail over the limit, this much is read: its headers, and no more.
pub const HEAD_BYTES: u64 = 64 * 1024;

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
    /// Stable within the account: mailbox and the server's own id.
    pub stable_id: String,
    /// `bytes` is only the start of a mail over the limit, whose size this is. It says who and what;
    /// the rest stays on the server.
    pub too_large: Option<u64>,
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
    /// Bytes per mail. Of a larger one only the start is read, so that a sender cannot fill the
    /// memory; the core keeps a stub, and the mail itself stays on the server.
    pub bytes: usize,
    /// Bytes per call, from the sizes the server reports, so that a sender cannot hold up an account
    /// with fifty large mails on a slow line. At least one mail goes through; `more` says the rest wait.
    pub batch_bytes: u64,
}

/// What one call may still take.
#[derive(Debug)]
pub(crate) struct Budget {
    left: u64,
    taken: bool,
}

impl Budget {
    pub(crate) fn new(bytes: u64) -> Self {
        Self {
            left: bytes,
            taken: false,
        }
    }

    /// Whether a mail of `size` fits. The first always does, or a large mail would never pass.
    pub(crate) fn admit(&mut self, size: u64) -> bool {
        if self.taken && size > self.left {
            return false;
        }
        self.left = self.left.saturating_sub(size);
        self.taken = true;
        true
    }
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
    /// What comes after `cursor`, within `limits`. May repeat a mail; never skips one.
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError>;
    /// Keep `raw` as the sent copy.
    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError>;
}
