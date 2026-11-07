//! Forward: a new message with the same text and files, to other people.
//!
//! Pattern: Facade over the send flow: forwarding is sending, so the same check holds and there is
//! no second way out. The conversation is new; the attachments are shared by reference.

use fs_objects::ObjectId;

use super::draft_data::DraftData;
use super::handle::Inner;
use crate::error::{Error, Result};

impl Inner {
    pub(super) async fn forward(
        &self,
        conversation: ObjectId,
        id: ObjectId,
        recipients: Vec<String>,
    ) -> Result<()> {
        let source = self.read_message(conversation, id).await?;
        if source.summary.undecrypted || source.summary.too_large > 0 {
            return Err(Error::Conflict(
                "this message has no readable content to forward".into(),
            ));
        }
        let mut data = DraftData::new();
        data.account = self.load_conversation(conversation).await?.content.account;
        data.to = recipients;
        data.text = source.body_text;
        data.media_type = source.body_media_type;
        data.attachments = source.attachments;
        self.send_draft(&data).await?;
        Ok(())
    }
}
