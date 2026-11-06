//! Sort keys and alias keys.
//!
//! Pattern: Strategy (pure functions): one canonical form for writing and querying. Keys are fixed
//! width and end in the id, so they are unique, and text order is time order; that is what makes
//! cursors stable.

use chrono::{DateTime, Utc};
use fs_store::ObjectId;

use crate::model::ExternalId;

/// Dates before 1970 (a mail can claim anything) sort as 1970.
fn micros(at: DateTime<Utc>) -> i64 {
    at.timestamp_micros().max(0)
}

/// Oldest first.
pub(super) fn order_key(at: DateTime<Utc>, id: ObjectId) -> String {
    format!("{:019}-{id}", micros(at))
}

/// Newest first.
pub(super) fn desc_key(at: DateTime<Utc>, id: ObjectId) -> String {
    format!("{:019}-{id}", i64::MAX - micros(at))
}

/// Pinned first, then newest first.
pub(super) fn conversation_key(pinned: bool, at: DateTime<Utc>, id: ObjectId) -> String {
    format!("{}{}", if pinned { "0" } else { "1" }, desc_key(at, id))
}

/// Scoped to the account (provisional): a sender in one account cannot hide a message in another
/// by claiming its `Message-ID`.
pub(super) fn alias_key(account: ObjectId, alias: &ExternalId) -> String {
    format!("{account}|{}|{}", alias.scheme, alias.id)
}
