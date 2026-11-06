//! What a draft holds, and its stored form.
//!
//! Pattern: Adapter. A draft is an object of its own type, so it never shows up among the messages;
//! when it is sent, a new `Note` is created, published at the time of sending.

use fs_store::{Envelope, ObjectId, Query, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{CONTENT_TYPE, DRAFT, IDX_BLOB, SENSITIVE, TEXT_PLAIN};
use super::handle::Inner;
use super::text::display_name;
use crate::draft::DraftInfo;
use crate::error::{Error, Result};
use crate::model::Attachment;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DraftData {
    pub(super) id: ObjectId,
    pub(super) account: Option<ObjectId>,
    pub(super) conversation: Option<ObjectId>,
    pub(super) reply_to: Option<ObjectId>,
    pub(super) to: Vec<String>,
    pub(super) cc: Vec<String>,
    pub(super) bcc: Vec<String>,
    pub(super) text: String,
    pub(super) media_type: String,
    pub(super) attachments: Vec<Attachment>,
}

impl DraftData {
    pub(super) fn new() -> Self {
        Self {
            id: ObjectId::new(),
            account: None,
            conversation: None,
            reply_to: None,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            text: String::new(),
            media_type: TEXT_PLAIN.to_string(),
            attachments: vec![],
        }
    }

    /// Everyone who would get it, hidden recipients too: all of them need a key.
    pub(super) fn recipients(&self) -> Vec<String> {
        [self.to.as_slice(), &self.cc, &self.bcc].concat()
    }

    fn to_object(&self) -> Result<StoredObject> {
        let mut env = Envelope::new(DRAFT, display_name(None, &self.text));
        env.id = self.id;
        env.to.clone_from(&self.to);
        env.cc.clone_from(&self.cc);
        env.bcc.clone_from(&self.bcc);
        env.sensitive = vec![SENSITIVE.to_string()];
        let mut o = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(self)?);
        for a in &self.attachments {
            o = o.with_index(IDX_BLOB, a.blob.to_string());
        }
        Ok(o)
    }
}

impl Inner {
    pub(super) async fn save_draft(&self, data: &DraftData) -> Result<()> {
        let _writes = self.writes.lock().await;
        self.put(data.to_object()?).await
    }

    pub(super) async fn load_draft(&self, id: ObjectId) -> Result<DraftData> {
        let o = self
            .store
            .get(id)
            .await?
            .filter(|o| o.envelope.object_type == DRAFT)
            .ok_or(Error::NotFound(id))?;
        Ok(serde_json::from_value(o.content)?)
    }

    pub(super) async fn list_drafts(&self) -> Result<Vec<DraftInfo>> {
        let objects = self.store.query(&Query::new().of_type(DRAFT)).await?;
        objects
            .into_iter()
            .map(|o| {
                let d: DraftData = serde_json::from_value(o.content)?;
                Ok(DraftInfo {
                    id: d.id,
                    to: d.to,
                    text: d.text,
                })
            })
            .collect()
    }

    /// Remove the stored draft, if it was ever saved. The attachments stay for whoever names them.
    pub(super) async fn drop_draft_object(&self, id: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        if self.store.get(id).await?.is_some() {
            self.store.delete(id, &self.config.actor).await?;
        }
        Ok(())
    }
}
