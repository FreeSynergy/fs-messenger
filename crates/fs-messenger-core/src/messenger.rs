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
    /// `conversation.sticky`.
    async fn pin(&self, _conversation: ObjectId, _pinned: bool) -> Result<()> {
        no("conversation.sticky")
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
    /// `conversation.export` (accepted idea 10, ADR 0046): the whole conversation in an open format, JSON
    /// Lines of AS2 objects, oldest first. Built from `messages` and `message`, so every provider that
    /// can read a conversation can export it. Messages that cannot be read are named in the error, not left out silently.
    async fn export_conversation(&self, conversation: ObjectId) -> Result<String> {
        let mut found: Vec<Message> = vec![];
        let mut cursor: Option<Cursor> = None;
        loop {
            let page = self.messages(conversation, cursor.as_ref()).await?;
            if let Some(skipped) = page.skipped.first() {
                return Err(crate::error::Error::Invalid(format!(
                    "{} messages could not be read; the export would be incomplete (first: {skipped})",
                    page.skipped.len()
                )));
            }
            for summary in &page.items {
                found.push(self.message(conversation, summary.envelope.id).await?);
            }
            match page.next {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        found.reverse();
        crate::export::conversation_jsonl(&found)
            .map_err(|e| crate::error::Error::Invalid(e.to_string()))
    }
    /// `conversation.whyPlain` (accepted idea 9): why this conversation is not end-to-end encrypted and
    /// what to do about it; `None` if it is. A value, not a sentence: the words are the frontend's (ADR 0057).
    async fn why_not_encrypted(
        &self,
        conversation: ObjectId,
    ) -> Result<Option<(crate::WhyNot, crate::Remedy)>> {
        let summary = self.open(conversation).await?;
        let account_can = self.accounts().await?.iter().any(|a| a.encryption_ready);
        let other_side_ready = matches!(
            summary.encryption,
            crate::model::Encryption::Mls
                | crate::model::Encryption::Openpgp
                | crate::model::Encryption::Megolm
        );
        Ok(crate::why_not(crate::EncryptionFacts {
            encryption: summary.encryption,
            other_side_ready,
            account_can,
        }))
    }
    /// `receipt.choice` (accepted idea 4): whether this person shows that they read. `Hidden` until they choose.
    async fn receipt_choice(&self) -> Result<crate::ReceiptChoice> {
        no("receipt.choice")
    }
    /// `receipt.choice`: choose.
    async fn set_receipt_choice(&self, _choice: crate::ReceiptChoice) -> Result<()> {
        no("receipt.choice")
    }
    /// Whether a read receipt between this person and one who chose `theirs` is sent and shown: only when
    /// both show. Providers ask this before they send one; a frontend asks it before it shows one.
    async fn show_receipt(&self, theirs: crate::ReceiptChoice) -> Result<bool> {
        Ok(crate::receipts_both_ways(
            self.receipt_choice().await?,
            theirs,
        ))
    }
    /// `message.hold` (accepted idea 1): keep a saved draft for `wait` seconds (default
    /// [`crate::COOL_DOWN_SECS`]) before it goes out. Answers the id of the held message.
    async fn hold(&self, _draft: ObjectId, _now: u64, _wait: Option<u64>) -> Result<ObjectId> {
        no("message.hold")
    }
    /// `message.hold`: where a held message stands.
    async fn held_stage(&self, _held: ObjectId) -> Result<crate::CoolDown> {
        no("message.hold")
    }
    /// `message.hold`: send it now. Answers the id of the message, or an error if it is not held any more.
    async fn send_held_now(&self, _held: ObjectId, _now: u64) -> Result<ObjectId> {
        no("message.hold")
    }
    /// `message.hold`: take it back while it is held; it is a draft again and never goes out.
    async fn recall_held(&self, _held: ObjectId) -> Result<()> {
        no("message.hold")
    }
    /// `message.hold`: send what is due at `now`. The host calls this on a timer; answers the held ids that went out.
    async fn release_due(&self, _now: u64) -> Result<Vec<ObjectId>> {
        no("message.hold")
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
