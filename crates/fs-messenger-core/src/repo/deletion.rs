//! Deleting a message: `Delete` + tombstone (`data/messages/technical.md`).
//!
//! Pattern: Command in two store steps. The object goes, and an AS2 `Tombstone` under the same id
//! keeps only the aliases and the place in the order, so a replay does not bring the message back
//! and `next_unread` can go on from it. The store's log keeps the old image until compaction.

use fs_store::{Envelope, ObjectId, StoredObject};

use super::consts::{CONTENT_TYPE, IDX_ALIAS, IDX_ORDER, SENSITIVE, TOMBSTONE};
use super::handle::Inner;
use crate::error::Result;

impl Inner {
    pub(super) async fn delete_message(&self, conversation: ObjectId, id: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let message = self.load_in(conversation, id).await?;
        let old = self
            .store
            .get(id)
            .await?
            .ok_or(crate::Error::NotFound(id))?;
        self.store.delete(id, &self.config.actor).await?;

        let mut env = Envelope::new(TOMBSTONE, "deleted message");
        env.id = id;
        env.context.clone_from(&old.envelope.context);
        env.sensitive = vec![SENSITIVE.to_string()];
        let mut tombstone = StoredObject::new(env, CONTENT_TYPE, serde_json::json!({}));
        for key in [IDX_ALIAS, IDX_ORDER] {
            if let Some(values) = old.index.get(key) {
                tombstone.index.insert(key.to_string(), values.clone());
            }
        }
        self.put(tombstone).await?;
        self.release_attachments(&message.attachments).await?;
        self.refresh(conversation).await
    }
}
