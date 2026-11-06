//! The type: the memory store, the journal, and what the memory store would forget.
//!
//! Pattern: Decorator (its state).

use std::sync::{Arc, PoisonError};

use matrix_sdk_base::crypto::olm::{PrivateCrossSigningIdentity, StaticAccountData};
use matrix_sdk_base::crypto::store::MemoryStore;
use tokio::sync::{Mutex, RwLock};

use crate::store::Journal;

/// A `CryptoStore` that survives a restart. Reads come from memory; every write reaches the journal
/// first, in one atomic step.
pub struct FsCryptoStore {
    pub(super) memory: MemoryStore,
    pub(super) journal: Arc<dyn Journal>,
    /// Kept here, because the memory store forgets it on any `save_changes` that does not carry it.
    pub(super) identity: RwLock<Option<PrivateCrossSigningIdentity>>,
    /// What never changes about the account, once there is one.
    pub(super) static_account: std::sync::RwLock<Option<StaticAccountData>>,
    /// One save at a time, so that the journal and memory see changes in the same order.
    pub(super) saving: Mutex<()>,
}

impl FsCryptoStore {
    /// The account's ids and public keys, if there is an account.
    pub fn get_static_account(&self) -> Option<StaticAccountData> {
        self.static_account
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn remember_account(&self, data: StaticAccountData) {
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
