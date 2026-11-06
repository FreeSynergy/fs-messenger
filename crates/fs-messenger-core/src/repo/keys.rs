//! Sort keys and alias keys.
//!
//! Pattern: Strategy (pure functions): one canonical form for writing and querying. Keys are fixed
//! width and end in the id, so they are unique, and text order is the wanted order; that is what
//! makes cursors stable.

use chrono::{DateTime, Utc};
use fs_store::ObjectId;

use crate::model::ExternalId;

/// Dates before 1970 (a mail can claim anything) sort as 1970.
fn micros(at: DateTime<Utc>) -> i64 {
    at.timestamp_micros().max(0)
}

fn inverted(id: ObjectId) -> String {
    format!("{:032x}", !id.as_uuid().as_u128())
}

/// By local arrival, first arrived first: the order "read up to" and "next unread" run over.
pub(super) fn arrival_key(arrival: u64, id: ObjectId) -> String {
    format!("{arrival:020}-{id}")
}

/// Last arrived first, to find the highest sequence when the program starts.
pub(super) fn arrival_desc_key(arrival: u64, id: ObjectId) -> String {
    format!("{:020}-{}", u64::MAX - arrival, inverted(id))
}

/// The arrival sequence inside an [`arrival_desc_key`].
pub(super) fn arrival_of_desc_key(key: &str) -> Option<u64> {
    key.split('-')
        .next()?
        .parse::<u64>()
        .ok()
        .map(|v| u64::MAX - v)
}

/// Display order, newest first. On equal times the last arrived is on top, the exact reverse of
/// arrival, so the top message really is the last one that arrived among equals.
pub(super) fn desc_key(at: DateTime<Utc>, arrival: u64, id: ObjectId) -> String {
    format!(
        "{:019}-{:020}-{}",
        i64::MAX - micros(at),
        u64::MAX - arrival,
        inverted(id)
    )
}

/// Pinned first, then newest first.
pub(super) fn conversation_key(pinned: bool, at: DateTime<Utc>, id: ObjectId) -> String {
    format!(
        "{}{:019}-{}",
        if pinned { "0" } else { "1" },
        i64::MAX - micros(at),
        inverted(id)
    )
}

/// Scoped to the account (provisional): a sender in one account cannot hide a message in another
/// by claiming its `Message-ID`.
pub(super) fn alias_key(account: ObjectId, alias: &ExternalId) -> String {
    format!("{account}|{}|{}", alias.scheme, alias.id)
}
