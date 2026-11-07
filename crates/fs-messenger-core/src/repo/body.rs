//! Large bodies: bytes in a store object of their own, the way attachments are.
//!
//! Pattern: Repository. Each change of a message logs a before and an after image of it, so a body
//! in `content` would be copied by every flag. An update image leaves out an object's original, so
//! the body sits there. `DESIGN.md` says what this does not solve.

use fs_objects::{Blob, Envelope, StoredObject};

use super::consts::{CONTENT_TYPE, DOCUMENT, SENSITIVE};
use super::handle::Inner;
use crate::error::{Error, Result};
use crate::model::Message;

/// Up to here a body stays in `content`.
const INLINE_BODY_BYTES: usize = 64 * 1024;

impl Inner {
    /// Before a new or replaced body is stored.
    pub(super) async fn externalise_body(&self, m: &mut Message) -> Result<()> {
        if m.body_text.len() <= INLINE_BODY_BYTES || m.stored.body_blob.is_some() {
            return Ok(());
        }
        let mut env = Envelope::new(DOCUMENT, "message body");
        env.sensitive = vec![SENSITIVE.to_string()];
        let id = env.id;
        let object =
            StoredObject::new(env, CONTENT_TYPE, serde_json::json!({})).with_original(Blob {
                media_type: m.body_media_type.clone(),
                bytes: m.body_text.clone().into_bytes(),
            });
        self.put(object).await?;
        m.stored.body_blob = Some(id);
        Ok(())
    }

    /// The whole text, for whoever reads the message. A body that has gone is store damage and says
    /// so: an empty text would read as a message that says nothing, and forward would send it.
    pub(super) async fn hydrate(&self, mut m: Message) -> Result<Message> {
        if let Some(id) = m.stored.body_blob {
            let blob = self.store.get(id).await?.and_then(|o| o.original);
            let blob = blob.ok_or(Error::BodyMissing {
                message: m.id(),
                body: id,
            })?;
            m.body_text = String::from_utf8_lossy(&blob.bytes).into_owned();
        }
        Ok(m)
    }

    /// After the message that held it has gone, or has a new one.
    pub(super) async fn release_body(&self, m: &Message) -> Result<()> {
        if let Some(id) = m.stored.body_blob {
            // Already gone is store damage, and what was wanted anyway: it must not fail a delete.
            match self.store.delete(id, &self.config.actor).await {
                Ok(_) | Err(fs_objects::Error::NotFound(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}
