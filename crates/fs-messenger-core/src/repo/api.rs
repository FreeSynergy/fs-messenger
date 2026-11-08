//! The contract traits over the store-backed parts.
//!
//! Pattern: Facade: only wiring; each method hands over to the part that does the work. `rules` and
//! `set_rules` stay at their explicit `Unsupported` defaults, and so does `stop` without a guard.

use std::sync::Arc;

use async_trait::async_trait;
use fs_interfaces::provider::{Capabilities, Unsupported};
use fs_objects::{Blob, ObjectId, Query};

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

impl MessengerRepo {
    /// Send the draft of a held message that was just taken. **A downgrade means the message left**
    /// (as for `Draft::send`): it stays `SentUnread` and is never sent a second time. Any other
    /// failure puts it back to held. A failure to record the sent message does not undo the sending.
    async fn finish_held(
        &self,
        held: ObjectId,
        draft: ObjectId,
        before: crate::CoolDown,
    ) -> Result<ObjectId> {
        let sent = match self.resume_draft(draft).await {
            Ok(mut d) => d.send(None).await,
            Err(e) => Err(e),
        };
        match sent {
            Ok(message) => {
                // The message is out; if the note cannot be written, the stage already says so.
                let _ = self.inner.held_sent(held, message).await;
                Ok(message)
            }
            Err(e @ crate::error::Error::EncryptionDowngraded { .. }) => Err(e),
            Err(e) => {
                self.inner.restore_held(held, before).await?;
                Err(e)
            }
        }
    }
}

#[async_trait]
impl Messenger for MessengerRepo {
    fn capabilities(&self) -> Capabilities {
        report(self.inner.guard().is_some())
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
        self.inner.read_message(conversation, id).await
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
    async fn receipt_choice(&self) -> Result<crate::ReceiptChoice> {
        self.inner.receipt_choice().await
    }
    async fn set_receipt_choice(&self, choice: crate::ReceiptChoice) -> Result<()> {
        self.inner.set_receipt_choice(choice).await
    }
    async fn hold(&self, draft: ObjectId, now: u64, wait: Option<u64>) -> Result<ObjectId> {
        self.inner
            .hold(draft, now, wait.unwrap_or(crate::COOL_DOWN_SECS))
            .await
    }
    async fn held_stage(&self, held: ObjectId) -> Result<crate::CoolDown> {
        self.inner.held_stage(held).await
    }
    async fn send_held_now(&self, held: ObjectId, now: u64) -> Result<ObjectId> {
        let Some((draft, before)) = self.inner.take_due(held, now, true).await? else {
            return Err(crate::error::Error::Conflict(
                "it is not held any more".into(),
            ));
        };
        self.finish_held(held, draft, before).await
    }
    async fn held_unsure(&self) -> Result<Vec<ObjectId>> {
        self.inner.unsure_held().await
    }
    async fn recall_held(&self, held: ObjectId) -> Result<()> {
        self.inner.recall_held(held).await
    }
    async fn release_due(&self, now: u64) -> Result<Vec<ObjectId>> {
        let mut went = vec![];
        let mut first_error = None;
        for held in self.inner.due_held(now).await? {
            let Some((draft, before)) = self.inner.take_due(held, now, false).await? else {
                continue;
            };
            // One message that cannot go (its draft is gone, a key is missing) must not hold the others
            // back; it stays held and is tried again at the next call.
            match self.finish_held(held, draft, before).await {
                Ok(_) => went.push(held),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) if went.is_empty() => Err(e),
            _ => Ok(went),
        }
    }
    async fn stop(&self, conversation: ObjectId) -> Result<()> {
        match self.inner.guard() {
            Some(guard) => guard.stop_conversation(conversation).await,
            None => Err(Unsupported::new("turn.stop", "no guard is installed").into()),
        }
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
            let conversation = context.parse().map_err(crate::Error::Store)?;
            // The conversation knows the account as it is now; the message as it was then.
            data.account = self
                .inner
                .load_conversation(conversation)
                .await?
                .content
                .account;
            data.conversation = Some(conversation);
            data.reply_to = Some(parent);
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
            self.inner.announcements.subscribe(),
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
