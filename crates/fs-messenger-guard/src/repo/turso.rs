//! [`TursoGuardRepository`]: the state in `fs-objects`, out of reach of undo.
//!
//! Pattern: Adapter. The state is written through `UnloggedObjects`, so it has no activity with a before
//! image, and undo, redo and the change feed cannot reach it. The log is log-only activities on an id
//! of its own, which no undo stack holds. The object is marked sensitive and the store must be
//! encrypted: it says who is invited and which keys count. Give the guard a store of its own
//! (`DESIGN.md`): the rules hold on a shared one too, as tested, but the hub pushes a store's log.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use fs_objects::traits::{ActivityLog, UnloggedObjects};
use fs_objects::{ActivityFilter, Envelope, ObjectId, StoredObject, TursoStore};
use serde_json::Value;

use super::traits::GuardRepository;
use crate::log::{GuardLogEntry, subject};

/// The fixed id of the state object.
pub const STATE_ID: &str = "01900000-0000-7000-8000-00000000a1d0";
const STATE_TYPE: &str = "freesynergy.net:GuardState";
const CONTENT_TYPE: &str = "application/json";
const SENSITIVE: &str = "pd:Communication";
const LOG_PREFIX: &str = "freesynergy.net:guard";

/// The state and the log in one Turso store.
pub struct TursoGuardRepository {
    store: Arc<TursoStore>,
    id: ObjectId,
    engine_claimed: AtomicBool,
}

/// The stores with a guard repository in this process, by address. A second `TursoStore` on the same
/// file is another object, which this cannot see; `DESIGN.md` says so.
static CLAIMED: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

fn address(store: &Arc<TursoStore>) -> usize {
    Arc::as_ptr(store).cast::<()>() as usize
}

impl Drop for TursoGuardRepository {
    fn drop(&mut self) {
        CLAIMED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&address(&self.store));
    }
}

impl TursoGuardRepository {
    /// On an encrypted store; anything else is refused, as for the messenger itself (ADR 0024).
    pub fn open(store: Arc<TursoStore>) -> Result<Self, String> {
        if !store.is_encrypted() {
            return Err("the store is not encrypted; the guard does not open on it".into());
        }
        let id = STATE_ID
            .parse()
            .map_err(|e: fs_objects::Error| e.to_string())?;
        let mut claimed = CLAIMED.lock().unwrap_or_else(PoisonError::into_inner);
        if !claimed.insert(address(&store)) {
            return Err("this store already has a guard".into());
        }
        drop(claimed);
        Ok(Self {
            store,
            id,
            engine_claimed: AtomicBool::new(false),
        })
    }
}

#[async_trait]
impl GuardRepository for TursoGuardRepository {
    async fn load(&self) -> Result<Option<Value>, String> {
        let Some(object) = self
            .store
            .read_unlogged(self.id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        // Read, and not a state: that is damage, from which the local recovery can go on.
        if object.envelope.object_type != STATE_TYPE {
            let found = object.envelope.object_type;
            return Ok(Some(serde_json::json!({ "notAState": found })));
        }
        Ok(Some(object.content))
    }

    async fn save(&self, record: &Value) -> Result<(), String> {
        let mut envelope = Envelope::new(STATE_TYPE, "AI guard state");
        envelope.id = self.id;
        envelope.sensitive = vec![SENSITIVE.to_string()];
        let object = StoredObject::new(envelope, CONTENT_TYPE, record.clone());
        self.store
            .put_unlogged(object)
            .await
            .map_err(|e| e.to_string())
    }

    async fn append(&self, entry: &GuardLogEntry) -> Result<(), String> {
        let payload = serde_json::json!({ LOG_PREFIX: entry });
        self.store
            .log(entry.kind(), subject(), &entry.actor, payload)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn claim_engine(&self) -> Result<(), String> {
        if self.engine_claimed.swap(true, Ordering::SeqCst) {
            return Err("this repository already has an engine".into());
        }
        Ok(())
    }

    fn store_address(&self) -> Option<usize> {
        Some(address(&self.store))
    }

    async fn entries(&self) -> Result<Vec<GuardLogEntry>, String> {
        let filter = ActivityFilter {
            object: Some(subject()),
            ..ActivityFilter::default()
        };
        let log = self
            .store
            .activities(&filter)
            .await
            .map_err(|e| e.to_string())?;
        Ok(log
            .into_iter()
            .filter_map(|a| {
                let mut entry: GuardLogEntry =
                    serde_json::from_value(a.payload.get(LOG_PREFIX)?.clone()).ok()?;
                entry.at = Some(a.at);
                entry.actor = a.actor;
                Some(entry)
            })
            .collect())
    }
}
