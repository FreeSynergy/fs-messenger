//! The record kinds of the state journal.
//!
//! Pattern: none (constants): one name per thing the store keeps.

pub(super) const ACCOUNT_DATA: &str = "account_data";
pub(super) const PROFILE: &str = "profile";
pub(super) const STATE: &str = "state";
pub(super) const STRIPPED: &str = "stripped";
pub(super) const ROOM_DATA: &str = "room_data";
pub(super) const ROOM_INFO: &str = "room_info";
pub(super) const REDACTION: &str = "redaction";
pub(super) const NAMES: &str = "names";
pub(super) const CUSTOM: &str = "custom";
pub(super) const SYNC_TOKEN: &str = "sync_token";
pub(super) const FILTER: &str = "filter";
pub(super) const ONE_TIME_KEY: &str = "one_time_key_uploaded";
/// What hangs below a room, by more than its id.
pub(super) const OF_ROOM: [&str; 6] = [PROFILE, STATE, STRIPPED, ROOM_DATA, REDACTION, NAMES];
