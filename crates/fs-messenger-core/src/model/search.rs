//! [`SearchFilter`].
//!
//! Pattern: Value object, the contract's `SearchFilter`.

use chrono::{DateTime, Utc};

/// What narrows a search. A `false` or `None` does not narrow anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// Part of the sender's address.
    pub from: Option<String>,
    /// Only unread messages.
    pub unread: bool,
    /// Only messages with attachments.
    pub has_attachment: bool,
    /// Only messages published at or after this time.
    pub since: Option<DateTime<Utc>>,
}
