//! Reading journal records back into the SDK's types.
//!
//! Pattern: Strategy (pure functions). The value of a record is not named in an error: it may be a key.

use matrix_sdk_base::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde::de::DeserializeOwned;

use crate::error::StoreFail;

/// One record's value.
pub(super) fn read<T: DeserializeOwned>(text: &str) -> Result<T, StoreFail> {
    Ok(serde_json::from_str(text)?)
}

/// A record of key material that cannot be read. Says which, and not why: a parser's reason can quote.
pub(super) fn unreadable(what: &str) -> StoreFail {
    StoreFail::Record(format!("{what}: cannot be read"))
}

/// A record that cannot be what it says. Only for ids, which are no secret.
pub(super) fn bad(what: &str, why: impl std::fmt::Display) -> StoreFail {
    StoreFail::Record(format!("{what}: {why}"))
}

/// A room id from a key.
pub(super) fn room(text: &str) -> Result<OwnedRoomId, StoreFail> {
    let id = <&RoomId>::try_from(text).map_err(|e| bad("room id", e))?;
    Ok(id.to_owned())
}

/// A user id from a key.
pub(super) fn user(text: &str) -> Result<OwnedUserId, StoreFail> {
    let id = <&UserId>::try_from(text).map_err(|e| bad("user id", e))?;
    Ok(id.to_owned())
}
