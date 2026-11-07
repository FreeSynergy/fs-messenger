//! The [`Messenger`] contract: what the program offers, the Rust form of `messenger.capnp`.
//!
//! Pattern: Repository (interface) + Null Object: callers depend on this trait, never on
//! `MessengerRepo`. Every method exists (ADR 0045); what a provider of this interface does not
//! override answers an explicit `Unsupported`, and `capabilities` reports the same. The contract's
//! `Conversation` capability is flattened to methods with the conversation's id.

use async_trait::async_trait;
use fs_interfaces::provider::{Capabilities, Unsupported};
use fs_objects::{Blob, ObjectId};

use crate::draft::{Draft, DraftInfo};
use crate::error::Result;
use crate::listener::{Listener, Subscription};
use crate::model::{
    Account, AccountSetup, ConversationPage, ConversationSummary, Cursor, EventFilter, Message,
    MessageAction, MessagePage, MessageSummary, SearchFilter, Usage,
};

/// The version of `messenger.capnp` this core mirrors.
pub const CONTRACT_VERSION: &str = "0.3";

fn no<T>(capability: &str) -> Result<T> {
    Err(Unsupported::new(capability, "this provider does not offer it").into())
}

/// One program for mail, chat and agents; encrypted only (ADR 0018).
///
/// Interface status: draft (ADR 0045).
#[async_trait]
pub trait Messenger: Send + Sync {
    /// What this program offers, per capability id.
    fn capabilities(&self) -> Capabilities;

    /// `account.list`.
    async fn accounts(&self) -> Result<Vec<Account>> {
        no("account.list")
    }
    /// `account.add`. A non-empty secret is `Unsupported`: there is no key vault yet (ADR 0024).
    async fn add_account(&self, _setup: AccountSetup) -> Result<Account> {
        no("account.add")
    }
    /// `account.remove`. Its messages stay.
    async fn remove_account(&self, _id: ObjectId) -> Result<()> {
        no("account.remove")
    }
    /// `conversation.list`: pinned first, then newest first.
    async fn conversations(
        &self,
        _folder: Option<&str>,
        _only_unread: bool,
        _cursor: Option<&Cursor>,
    ) -> Result<ConversationPage> {
        no("conversation.list")
    }
    /// `conversation.open`.
    async fn open(&self, _id: ObjectId) -> Result<ConversationSummary> {
        no("conversation.open")
    }
    /// `conversation.nextUnread`: the oldest unread message over all conversations, after `after`.
    async fn next_unread(&self, _after: Option<ObjectId>) -> Result<Option<MessageSummary>> {
        no("conversation.nextUnread")
    }
    /// `conversation.open`: messages of a conversation, newest first.
    async fn messages(
        &self,
        _conversation: ObjectId,
        _cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        no("conversation.open")
    }
    /// `message.open`.
    async fn message(&self, _conversation: ObjectId, _id: ObjectId) -> Result<Message> {
        no("message.open")
    }
    /// `conversation.markRead`: read up to and including `up_to`, by message order.
    async fn mark_read(&self, _conversation: ObjectId, _up_to: ObjectId) -> Result<()> {
        no("conversation.markRead")
    }
    /// `conversation.pin`.
    async fn pin(&self, _conversation: ObjectId, _pinned: bool) -> Result<()> {
        no("conversation.pin")
    }
    /// `conversation.move`: into this folder only; empty takes it out of all folders.
    async fn move_to(&self, _conversation: ObjectId, _folder: &str) -> Result<()> {
        no("conversation.move")
    }
    /// One action on one message. Answers an error if nothing was done.
    async fn act(
        &self,
        _conversation: ObjectId,
        _id: ObjectId,
        _action: MessageAction,
    ) -> Result<()> {
        no("message.act")
    }
    /// `turn.stop`: ends the running agent turns of a conversation.
    async fn stop(&self, _conversation: ObjectId) -> Result<()> {
        no("turn.stop")
    }
    /// `conversation.usage`: the total over this conversation.
    async fn usage(&self, _conversation: ObjectId) -> Result<Usage> {
        no("conversation.usage")
    }
    /// `message.search`: substring over subject and text, newest first.
    async fn search(
        &self,
        _text: &str,
        _filter: &SearchFilter,
        _cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        no("message.search")
    }
    /// `message.compose`: a new draft; with `reply_to`, an answer to that message.
    async fn compose(&self, _reply_to: Option<ObjectId>) -> Result<Box<dyn Draft>> {
        no("message.compose")
    }
    /// `draft.list`.
    async fn drafts(&self) -> Result<Vec<DraftInfo>> {
        no("draft.list")
    }
    /// `draft.list`: pick a saved draft up again.
    async fn resume_draft(&self, _id: ObjectId) -> Result<Box<dyn Draft>> {
        no("draft.list")
    }
    /// `attachment.save`: the bytes behind `Attachment.blob`.
    async fn attachment(&self, _blob: ObjectId) -> Result<Blob> {
        no("attachment.save")
    }
    /// `rule.read`: the rules as Sieve.
    async fn rules(&self) -> Result<String> {
        no("rule.read")
    }
    /// `rule.write`.
    async fn set_rules(&self, _sieve: &str) -> Result<()> {
        no("rule.write")
    }
    /// `event.subscribe`: subscribe before fetching. Dropping the answer ends it.
    async fn subscribe(
        &self,
        _filter: EventFilter,
        _listener: std::sync::Arc<dyn Listener>,
    ) -> Result<Subscription> {
        no("event.subscribe")
    }
}
