//! A stored message that could not be decrypted gets the readable version.
//!
//! Pattern: State: `undecrypted` -> readable, only ever in this direction and only by an id the
//! provider issued. The original stays as it is (ADR 0030); text, name, preview, search, encryption
//! and signature follow what can now be read.

use fs_objects::ObjectId;

use super::handle::Inner;
use super::message_codec::{message_from, message_to};
use super::text::{body_media_type, display_name, preview};
use crate::error::Result;
use crate::provider::RawMessage;

impl Inner {
    pub(super) async fn upgrade_undecrypted(&self, id: ObjectId, raw: RawMessage) -> Result<()> {
        if raw.undecrypted {
            return Ok(());
        }
        let Some(object) = self.store.get(id).await? else {
            return Ok(());
        };
        // A tombstone or a record this version cannot read is not touched.
        let Ok(mut m) = message_from(object) else {
            return Ok(());
        };
        if !m.summary.undecrypted || m.summary.too_large > 0 {
            return Ok(());
        }
        let old = m.clone();
        let s = &mut m.summary;
        s.preview = preview(&raw.body_text);
        s.envelope.name = display_name(raw.subject.as_deref(), &raw.body_text);
        s.undecrypted = false;
        s.signature = raw.signature;
        s.encryption = raw.encryption;
        m.body_media_type = body_media_type(&raw.body_media_type);
        m.body_text = raw.body_text;
        m.stored.body_blob = None;
        m.stored.indexed_text = None;
        for a in raw.attachments {
            let stored = self.store_attachment(a.name, a.media_type, a.data).await?;
            m.attachments.push(stored);
        }
        self.externalise_body(&mut m).await?;
        self.put(message_to(&m)?).await?;
        self.release_body(&old).await?;
        if let Some(conversation) = m.summary.envelope.context.as_deref() {
            self.refresh(conversation.parse()?).await?;
        }
        Ok(())
    }
}
