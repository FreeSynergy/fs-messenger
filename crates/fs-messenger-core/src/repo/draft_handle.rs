//! [`StoredDraft`]: the [`Draft`] contract over the store.
//!
//! Pattern: State: open until a send succeeds or it is discarded. A refused or failed send leaves it
//! open and saved, so the person can get a key and try again.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs_interfaces::provider::Unsupported;
use fs_store::ObjectId;

use super::consts::{TEXT_MARKDOWN, TEXT_PLAIN};
use super::draft_data::DraftData;
use super::handle::Inner;
use crate::draft::Draft;
use crate::error::{Error, Result};
use crate::gate;
use crate::ingest_rules::printable;
use crate::model::EncryptionPlan;

pub(super) struct StoredDraft {
    inner: Arc<Inner>,
    data: DraftData,
    open: bool,
}

impl StoredDraft {
    pub(super) fn new(inner: Arc<Inner>, data: DraftData) -> Self {
        Self {
            inner,
            data,
            open: true,
        }
    }

    fn check_open(&self) -> Result<()> {
        if self.open {
            Ok(())
        } else {
            Err(Error::Conflict("this draft was sent or discarded".into()))
        }
    }

    async fn plan(&self) -> Result<EncryptionPlan> {
        self.inner
            .plan_for(self.data.account, &self.data.recipients())
            .await
    }
}

#[async_trait]
impl Draft for StoredDraft {
    async fn set_recipients(
        &mut self,
        to: Vec<String>,
        cc: Vec<String>,
        bcc: Vec<String>,
    ) -> Result<Vec<String>> {
        self.check_open()?;
        gate::check_recipients(&[to.as_slice(), &cc, &bcc].concat())?;
        self.data.to = to;
        self.data.cc = cc;
        self.data.bcc = bcc;
        Ok(self.plan().await?.missing_keys)
    }

    async fn set_account(&mut self, account: ObjectId) -> Result<()> {
        self.check_open()?;
        if self.data.conversation.is_some() {
            let why = "this draft answers in a conversation, which has its own account";
            return Err(Error::Conflict(why.into()));
        }
        self.inner.load_account(account).await?;
        self.data.account = Some(account);
        Ok(())
    }

    async fn set_text(&mut self, text: String, media_type: String) -> Result<()> {
        self.check_open()?;
        if media_type != TEXT_PLAIN && media_type != TEXT_MARKDOWN {
            return Err(Error::Invalid(format!(
                "{media_type} is not a draft format"
            )));
        }
        self.data.text = text.replace('\0', "");
        self.data.media_type = media_type;
        Ok(())
    }

    async fn set_subject(&mut self, subject: String) -> Result<()> {
        self.check_open()?;
        self.data.subject = printable(&subject);
        Ok(())
    }

    async fn attach(&mut self, name: String, media_type: String, data: Vec<u8>) -> Result<()> {
        self.check_open()?;
        let a = self.inner.store_attachment(name, media_type, data).await?;
        self.data.attachments.push(a);
        self.inner.save_draft(&self.data).await
    }

    async fn encryption_plan(&self) -> Result<EncryptionPlan> {
        self.check_open()?;
        self.plan().await
    }

    async fn save(&mut self) -> Result<ObjectId> {
        self.check_open()?;
        self.inner.save_draft(&self.data).await?;
        Ok(self.data.id)
    }

    async fn send(&mut self, at: Option<DateTime<Utc>>) -> Result<ObjectId> {
        self.check_open()?;
        self.inner.save_draft(&self.data).await?;
        if at.is_some_and(|t| t > Utc::now()) {
            let why = "nothing runs a scheduled send yet; the draft is saved and nothing left";
            return Err(Unsupported::new("message.schedule", why).into());
        }
        let sent = self.inner.send_draft(&self.data).await;
        // A downgrade means it left: another try would send it again.
        if matches!(sent, Ok(_) | Err(Error::EncryptionDowngraded { .. })) {
            self.open = false;
        }
        sent
    }

    async fn discard(&mut self) -> Result<()> {
        self.check_open()?;
        self.inner.drop_draft_object(self.data.id).await?;
        let _writes = self.inner.writes.lock().await;
        self.inner
            .release_attachments(&self.data.attachments)
            .await?;
        self.open = false;
        Ok(())
    }
}
