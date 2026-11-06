//! [`FsCryptoStore`]: the SDK's `CryptoStore`: Olm account, sessions, Megolm keys, device lists.
//!
//! Pattern: Decorator over the SDK's `MemoryStore`: `persist.rs` turns a change into journal entries,
//! `restore.rs` rebuilds the memory store from them, `decorate.rs` is the trait.

mod decorate;
mod persist;
mod restore;

pub(crate) use persist::base64_free::{decode_bytes, encode_bytes};

use std::sync::{Arc, PoisonError};

use matrix_sdk_base::crypto::olm::{PrivateCrossSigningIdentity, StaticAccountData};
use matrix_sdk_base::crypto::store::MemoryStore;
use tokio::sync::{Mutex, RwLock};

use super::Journal;

/// Record kinds of this store.
mod kind {
    pub(super) const ACCOUNT: &str = "account";
    pub(super) const IDENTITY: &str = "identity";
    pub(super) const BACKUP_VERSION: &str = "backup_version";
    pub(super) const BACKUP_KEY: &str = "backup_key";
    pub(super) const DEHYDRATED: &str = "dehydrated";
    pub(super) const SESSION: &str = "session";
    pub(super) const HASH: &str = "hash";
    pub(super) const INBOUND: &str = "inbound";
    pub(super) const INBOUND_BACKED_UP: &str = "inbound_backed_up";
    pub(super) const OUTBOUND: &str = "outbound";
    pub(super) const REQUEST: &str = "request";
    pub(super) const USER_IDENTITY: &str = "user_identity";
    pub(super) const DEVICE: &str = "device";
    pub(super) const WITHHELD: &str = "withheld";
    pub(super) const SETTINGS: &str = "settings";
    pub(super) const SECRET: &str = "secret";
    pub(super) const NEXT_BATCH: &str = "next_batch";
    pub(super) const BUNDLE: &str = "bundle";
    pub(super) const DOWNLOADED: &str = "downloaded";
    pub(super) const PENDING_BUNDLE: &str = "pending_bundle";
    pub(super) const TRACKED: &str = "tracked";
    pub(super) const CUSTOM: &str = "custom";
}

/// A `CryptoStore` that survives a restart. Reads come from memory; every write reaches the journal
/// first, in one atomic step.
pub struct FsCryptoStore {
    memory: MemoryStore,
    journal: Arc<dyn Journal>,
    /// Kept here, because the memory store forgets it on any `save_changes` that does not carry it.
    identity: RwLock<Option<PrivateCrossSigningIdentity>>,
    /// What never changes about the account, once there is one.
    static_account: std::sync::RwLock<Option<StaticAccountData>>,
    /// One save at a time, so that the journal and memory see changes in the same order.
    saving: Mutex<()>,
}

impl FsCryptoStore {
    /// The account's ids and public keys, if there is an account.
    pub fn get_static_account(&self) -> Option<StaticAccountData> {
        self.static_account
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn remember_account(&self, data: StaticAccountData) {
        *self
            .static_account
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(data);
    }
}

impl std::fmt::Debug for FsCryptoStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FsCryptoStore(<keys are not shown>)")
    }
}
