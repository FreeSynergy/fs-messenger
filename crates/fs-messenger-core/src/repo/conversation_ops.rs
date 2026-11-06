//! What a person changes about a conversation: pin, folder; and its usage.
//!
//! Pattern: Command: small edits of one stored record. They are local: no provider is told.

use fs_store::{ActivityKind, LogEntry, ObjectId, StoredObject};

use super::handle::Inner;
use crate::error::Result;
use crate::model::Usage;

impl Inner {
    async fn put_logged(
        &self,
        o: StoredObject,
        kind: ActivityKind,
        why: serde_json::Value,
    ) -> Result<()> {
        let extra = vec![LogEntry::new(kind, why)];
        self.store.put_and_log(o, &self.config.actor, extra).await?;
        Ok(())
    }

    pub(super) async fn pin(&self, id: ObjectId, pinned: bool) -> Result<()> {
        let _writes = self.writes.lock().await;
        let mut record = self.load_conversation(id).await?;
        record.content.pinned = pinned;
        self.put(record.to_object()?).await
    }

    pub(super) async fn move_to(&self, id: ObjectId, folder: &str) -> Result<()> {
        let _writes = self.writes.lock().await;
        let mut record = self.load_conversation(id).await?;
        record.content.folders = if folder.is_empty() {
            vec![]
        } else {
            vec![folder.to_string()]
        };
        let why = serde_json::json!({ "folder": folder });
        self.put_logged(record.to_object()?, ActivityKind::Move, why)
            .await
    }

    pub(super) async fn usage(&self, id: ObjectId) -> Result<Usage> {
        Ok(self.load_conversation(id).await?.content.usage)
    }

    pub(super) async fn record_usage(&self, id: ObjectId, usage: Usage) -> Result<()> {
        let _writes = self.writes.lock().await;
        let mut record = self.load_conversation(id).await?;
        record.content.usage = usage;
        self.put(record.to_object()?).await
    }
}
