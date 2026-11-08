//! [`FsJournal`]: the [`Journal`] over an encrypted `TursoStore`.
//!
//! Pattern: Adapter. One `append` is one object (a segment), written by `put_unlogged` in one
//! transaction: atomic, and with no old image in the activity log, which would keep every earlier
//! ratchet state (see `DESIGN.md`). From time to time all segments are folded into a base segment;
//! whatever lies before the newest base is ignored, so a crash during folding loses nothing.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use fs_objects::traits::UnloggedObjects;
use fs_objects::{Envelope, Query, StoredObject, TursoStore};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::journal::{Journal, Op, apply};
use crate::error::StoreFail;
use crate::secret::SecretText;

const SEGMENT: &str = "freesynergy.net:MatrixStoreSegment";
const SENSITIVE: &str = "pd:Communication";
const CONTENT_TYPE: &str = "application/json";
const IDX_JOURNAL: &str = "journal";
const IDX_SEQ: &str = "seq";
/// Segments written before they are folded. Folding reads everything, so not too often.
const FOLD_AFTER: u64 = 512;

#[derive(Serialize, Deserialize)]
struct Segment {
    /// Nothing before this segment counts.
    base: bool,
    ops: Vec<Op>,
}

#[derive(Default)]
struct Position {
    next: u64,
    since_fold: u64,
}

/// One named journal inside an encrypted store. Keep one handle per name.
pub struct FsJournal {
    store: Arc<TursoStore>,
    name: String,
    position: Mutex<Position>,
}

impl std::fmt::Debug for FsJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsJournal")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl FsJournal {
    /// Open the journal `name`. Refuses every store that is not encrypted, also one that opted out of
    /// the sensitive-data rule.
    pub async fn open(store: Arc<TursoStore>, name: impl Into<String>) -> Result<Self, StoreFail> {
        if !store.is_encrypted() {
            return Err(StoreFail::NotEncrypted);
        }
        let journal = Self {
            store,
            name: name.into(),
            position: Mutex::new(Position::default()),
        };
        let found = journal.segments().await?;
        let next = found.last().map_or(Ok(0), |o| seq_of(o).map(|s| s + 1))?;
        *journal.position.lock().await = Position {
            next,
            since_fold: found.len() as u64,
        };
        Ok(journal)
    }

    async fn segments(&self) -> Result<Vec<StoredObject>, StoreFail> {
        let q = Query::new()
            .of_type(SEGMENT)
            .with_index(IDX_JOURNAL, self.name.clone())
            .order_by_index(IDX_SEQ);
        Ok(self.store.query_unlogged(&q, &[SEGMENT]).await?)
    }

    async fn write(&self, seq: u64, segment: &Segment) -> Result<(), StoreFail> {
        let mut envelope = Envelope::new(SEGMENT, format!("{} {seq}", self.name));
        envelope.sensitive = vec![SENSITIVE.to_string()];
        let object = StoredObject::new(envelope, CONTENT_TYPE, serde_json::to_value(segment)?)
            .with_index(IDX_JOURNAL, self.name.clone())
            .with_index(IDX_SEQ, format!("{seq:020}"));
        Ok(self.store.put_unlogged(object).await?)
    }

    /// Fold everything into one base segment, then remove what it replaces.
    async fn fold(&self, position: &mut Position) -> Result<(), StoreFail> {
        let old = self.segments().await?;
        let merged = merge(&old)?;
        let ops = merged.into_iter().map(|(k, v)| Op::Put(k, v)).collect();
        self.write(position.next, &Segment { base: true, ops })
            .await?;
        position.next += 1;
        position.since_fold = 0;
        for object in old {
            self.store.delete_unlogged(object.envelope.id).await?;
        }
        Ok(())
    }
}

fn seq_of(object: &StoredObject) -> Result<u64, StoreFail> {
    object
        .index
        .get(IDX_SEQ)
        .and_then(|v| v.first())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| StoreFail::Record("a journal segment without a sequence".into()))
}

fn merge(objects: &[StoredObject]) -> Result<BTreeMap<String, SecretText>, StoreFail> {
    let mut map = BTreeMap::new();
    for object in objects {
        let segment: Segment = serde_json::from_value(object.content.clone())?;
        if segment.base {
            map.clear();
        }
        apply(&mut map, segment.ops);
    }
    Ok(map)
}

#[async_trait]
impl Journal for FsJournal {
    async fn append(&self, ops: Vec<Op>) -> Result<(), StoreFail> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut position = self.position.lock().await;
        self.write(position.next, &Segment { base: false, ops })
            .await?;
        position.next += 1;
        position.since_fold += 1;
        if position.since_fold >= FOLD_AFTER {
            self.fold(&mut position).await?;
        }
        Ok(())
    }

    async fn load(&self) -> Result<BTreeMap<String, SecretText>, StoreFail> {
        let _position = self.position.lock().await;
        merge(&self.segments().await?)
    }
}
