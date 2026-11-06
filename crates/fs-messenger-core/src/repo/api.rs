//! The contract traits over the store-backed parts.
//!
//! Pattern: Facade: only wiring; each method hands over to the part that does the work. `stop`,
//! `rules` and `set_rules` stay at their explicit `Unsupported` defaults.

use std::sync::Arc;

use async_trait::async_trait;
use fs_interfaces::provider::Capabilities;
use fs_store::{Blob, ObjectId, Query};

use super::capabilities::report;
use super::consts::ACCOUNT;
use super::draft_data::DraftData;
use super::draft_handle::StoredDraft;
use super::handle::MessengerRepo;
use super::subscription;
use crate::draft::{Draft, DraftInfo};
use crate::error::Result;
use crate::ingest::{Ingest, IngestSteps, Ingested, SyncReport};
use crate::listener::{Listener, Subscription};
use crate::messenger::Messenger;
use crate::model::{
    Account, AccountSetup, ConversationPage, ConversationSummary, Cursor, EventFilter, Message,
    MessageAction, MessagePage, MessageSummary, SearchFilter, ToolCall, Usage,
};
use crate::provider::RawMessage;

#[async_trait]
impl Messenger for MessengerRepo {
    fn capabilities(&self) -> Capabilities {
        report()
    }
    async fn accounts(&self) -> Result<Vec<Account>> {
        self.inner.list_accounts().await
    }
    async fn add_account(&self, setup: AccountSetup) -> Result<Account> {
        self.inner.add_account(setup).await
    }
    async fn remove_account(&self, id: ObjectId) -> Result<()> {
        self.inner.remove_account(id).await
    }
    async fn conversations(
        &self,
        folder: Option<&str>,
        only_unread: bool,
        cursor: Option<&Cursor>,
    ) -> Result<ConversationPage> {
        self.inner
            .list_conversations(folder, only_unread, cursor)
            .await
    }
    async fn open(&self, id: ObjectId) -> Result<ConversationSummary> {
        self.inner.open_conversation(id).await
    }
    async fn next_unread(&self, after: Option<ObjectId>) -> Result<Option<MessageSummary>> {
        self.inner.next_unread(after).await
    }
    async fn messages(
        &self,
        conversation: ObjectId,
        cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        self.inner.messages_page(conversation, cursor).await
    }
    async fn message(&self, conversation: ObjectId, id: ObjectId) -> Result<Message> {
        self.inner.load_in(conversation, id).await
    }
    async fn mark_read(&self, conversation: ObjectId, up_to: ObjectId) -> Result<()> {
        self.inner.mark_read(conversation, up_to).await
    }
    async fn pin(&self, conversation: ObjectId, pinned: bool) -> Result<()> {
        self.inner.pin(conversation, pinned).await
    }
    async fn move_to(&self, conversation: ObjectId, folder: &str) -> Result<()> {
        self.inner.move_to(conversation, folder).await
    }
    async fn act(&self, conversation: ObjectId, id: ObjectId, action: MessageAction) -> Result<()> {
        self.inner.act(conversation, id, action).await
    }
    async fn usage(&self, conversation: ObjectId) -> Result<Usage> {
        self.inner.usage(conversation).await
    }
    async fn search(
        &self,
        text: &str,
        filter: &SearchFilter,
        cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        self.inner.search(text, filter, cursor).await
    }
    async fn compose(&self, reply_to: Option<ObjectId>) -> Result<Box<dyn Draft>> {
        let mut data = DraftData::new();
        if let Some(parent) = reply_to {
            let m = self.inner.load_note(parent).await?;
            let context = m.summary.envelope.context.as_deref().unwrap_or_default();
            data.conversation = Some(context.parse().map_err(crate::Error::Store)?);
            data.reply_to = Some(parent);
            data.account = m.account;
            data.to = if m.outgoing {
                m.summary.envelope.to.clone()
            } else {
                m.summary.envelope.attributed_to.iter().cloned().collect()
            };
        } else {
            let accounts = self
                .inner
                .store
                .query(&Query::new().of_type(ACCOUNT).limit(2))
                .await?;
            if let [only] = accounts.as_slice() {
                data.account = Some(only.envelope.id);
            }
        }
        Ok(Box::new(StoredDraft::new(self.inner.clone(), data)))
    }
    async fn drafts(&self) -> Result<Vec<DraftInfo>> {
        self.inner.list_drafts().await
    }
    async fn resume_draft(&self, id: ObjectId) -> Result<Box<dyn Draft>> {
        let data = self.inner.load_draft(id).await?;
        Ok(Box::new(StoredDraft::new(self.inner.clone(), data)))
    }
    async fn attachment(&self, blob: ObjectId) -> Result<Blob> {
        self.inner.attachment_blob(blob).await
    }
    async fn subscribe(
        &self,
        filter: EventFilter,
        listener: Arc<dyn Listener>,
    ) -> Result<Subscription> {
        Ok(subscription::start(
            self.inner.store.clone(),
            filter,
            listener,
        ))
    }
}

#[async_trait]
impl Ingest for MessengerRepo {
    async fn ingest(&self, account: ObjectId, raw: RawMessage) -> Result<Ingested> {
        self.inner.ingest_flow(account, raw).await
    }
    async fn sync(&self, account: ObjectId) -> Result<SyncReport> {
        self.inner.sync(account).await
    }
    async fn record_tool_call(
        &self,
        message: ObjectId,
        agent_ref: &str,
        call: ToolCall,
    ) -> Result<ToolCall> {
        self.inner.record_tool_call(message, agent_ref, call).await
    }
    async fn record_usage(&self, conversation: ObjectId, usage: Usage) -> Result<()> {
        self.inner.record_usage(conversation, usage).await
    }
}
