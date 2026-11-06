//! The type: the memory store and the journal.
//!
//! Pattern: Decorator (its state).

use std::sync::Arc;

use matrix_sdk_base::store::MemoryStore;

use crate::store::Journal;

/// A `StateStore` that survives a restart. Reads come from memory; a save reaches the journal first.
pub struct FsStateStore {
    pub(super) memory: MemoryStore,
    pub(super) journal: Arc<dyn Journal>,
}

impl std::fmt::Debug for FsStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FsStateStore(..)")
    }
}
