//! Attachments: bytes in a store object of their own.
//!
//! Pattern: Repository. The bytes are the object's immutable original (provisional, see `DESIGN.md`),
//! so messages stay small. Messages and drafts name them in the index, and an attachment goes when
//! the last one that names it has gone.

use fs_store::{Blob, Envelope, ObjectId, Query, StoredObject};

use super::consts::{CONTENT_TYPE, DOCUMENT, IDX_BLOB, SENSITIVE};
use super::handle::Inner;
use crate::error::{Error, Result};
use crate::model::Attachment;
use crate::provider::OutgoingAttachment;

const OCTETS: &str = "application/octet-stream";

impl Inner {
    pub(super) async fn store_attachment(
        &self,
        name: String,
        media_type: String,
        data: Vec<u8>,
    ) -> Result<Attachment> {
        // A name is shown and saved under; a sender writes it.
        let printable = |t: String| t.chars().filter(|c| !c.is_control()).collect::<String>();
        let (name, media_type) = (printable(name), printable(media_type));
        let media_type = if media_type.is_empty() {
            OCTETS.to_string()
        } else {
            media_type
        };
        let shown = if name.trim().is_empty() {
            "attachment"
        } else {
            name.as_str()
        };
        let mut env = Envelope::new(DOCUMENT, shown);
        env.sensitive = vec![SENSITIVE.to_string()];
        let blob = env.id;
        let size = data.len() as u64;
        let content = serde_json::json!({ "name": name, "mediaType": media_type, "size": size });
        let object = StoredObject::new(env, CONTENT_TYPE, content).with_original(Blob {
            media_type: media_type.clone(),
            bytes: data,
        });
        self.put(object).await?;
        Ok(Attachment {
            name,
            media_type,
            size,
            blob,
        })
    }

    pub(super) async fn attachment_blob(&self, blob: ObjectId) -> Result<Blob> {
        self.store
            .get(blob)
            .await?
            .filter(|o| o.envelope.object_type == DOCUMENT)
            .and_then(|o| o.original)
            .ok_or(Error::NotFound(blob))
    }

    /// Call after the object that named them is gone.
    pub(super) async fn release_attachments(&self, attachments: &[Attachment]) -> Result<()> {
        for a in attachments {
            let users = Query::new().with_index(IDX_BLOB, a.blob.to_string());
            if self.store.count(&users).await? == 0 {
                self.store.delete(a.blob, &self.config.actor).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn outgoing_attachments(
        &self,
        attachments: &[Attachment],
    ) -> Result<Vec<OutgoingAttachment>> {
        let mut out = Vec::with_capacity(attachments.len());
        for a in attachments {
            out.push(OutgoingAttachment {
                name: a.name.clone(),
                media_type: a.media_type.clone(),
                data: self.attachment_blob(a.blob).await?.bytes,
            });
        }
        Ok(out)
    }
}
