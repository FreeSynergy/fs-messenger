//! [`GuardRepository`]: what the engine needs from a store.
//!
//! Pattern: Repository: the state as one opaque record, and an append-only log. The engine decides what
//! a record means, so this trait knows nothing of the rules, and whatever comes back from it is
//! checked. A failure to load reads as stopped.

use async_trait::async_trait;
use serde_json::Value;

use crate::log::GuardLogEntry;

/// The guard's persistence.
#[async_trait]
pub trait GuardRepository: Send + Sync {
    /// The stored record; `None` if there is none yet. `Err` is "cannot read".
    async fn load(&self) -> Result<Option<Value>, String>;
    /// Replace the record, atomically. It must not be possible to undo this.
    async fn save(&self, record: &Value) -> Result<(), String>;
    /// Add one entry to the log. It must not be possible to undo this.
    async fn append(&self, entry: &GuardLogEntry) -> Result<(), String>;
    /// The log, oldest first.
    async fn entries(&self) -> Result<Vec<GuardLogEntry>, String>;
}
