//! [`FsStateStore`]: the SDK's `StateStore`: rooms, members, room state, the sync token.
//!
//! Pattern: Decorator over the SDK's memory store: `persist.rs` turns a change into journal entries,
//! `restore.rs` rebuilds the memory store, `decorate.rs` is the trait. What is not kept across a
//! restart is listed in `DESIGN.md`: caches the SDK fills again, and features this crate does not use.

mod decorate;
mod persist;
mod restore;

use std::sync::Arc;

use matrix_sdk_base::store::MemoryStore;

use super::Journal;

/// Record kinds of this store.
mod kind {
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
    /// Everything that hangs on a room, for removing it.
    pub(super) const OF_ROOM: [&str; 7] = [
        PROFILE, STATE, STRIPPED, ROOM_DATA, ROOM_INFO, REDACTION, NAMES,
    ];
}

/// A `StateStore` that survives a restart. Reads come from memory; a save reaches the journal first.
pub struct FsStateStore {
    memory: MemoryStore,
    journal: Arc<dyn Journal>,
}

impl std::fmt::Debug for FsStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FsStateStore(..)")
    }
}
