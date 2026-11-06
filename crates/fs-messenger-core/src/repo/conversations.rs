//! Conversations: list, open, pin, move, and keeping their derived fields true.
//!
//! Pattern: Repository. `refresh` recomputes count, newest message and order from the messages, so
//! a crash between two writes heals with the next change.

use fs_store::{Envelope, ObjectId, Op, Query};

use super::consts::{
    CONVERSATION, IDX_CONV_ALIAS, IDX_DESC, IDX_FOLDER, IDX_SORT, IDX_UNREAD, NOTE, YES,
};
use super::conversation_codec::{ConversationContent, ConversationRecord};
use super::handle::Inner;
use super::keys::alias_key;
use super::paging::split_page;
use crate::error::{Error, Result};
use crate::model::{
    ConversationPage, ConversationSummary, Cursor, Encryption, ExternalId, Kind, Page,
};

impl Inner {
    pub(super) async fn load_conversation(&self, id: ObjectId) -> Result<ConversationRecord> {
        let o = self
            .store
            .get(id)
            .await?
            .filter(|o| o.envelope.object_type == CONVERSATION)
            .ok_or(Error::NotFound(id))?;
        ConversationRecord::from_object(o)
    }

    /// The conversation with this alias, if there is one.
    pub(super) async fn find_conversation(
        &self,
        scope: &str,
        alias: &ExternalId,
    ) -> Result<Option<ConversationRecord>> {
        let query = Query::new()
            .of_type(CONVERSATION)
            .with_index(IDX_CONV_ALIAS, alias_key(scope, alias))
            .limit(1);
        match self.store.query(&query).await?.pop() {
            Some(o) => Ok(Some(ConversationRecord::from_object(o)?)),
            None => Ok(None),
        }
    }

    /// The conversation with this alias, or a new one. One found from an earlier life of the account
    /// goes over to the account as it is now.
    pub(super) async fn ensure_conversation(
        &self,
        account: ObjectId,
        scope: &str,
        alias: &ExternalId,
        name: &str,
        kind: Kind,
    ) -> Result<ConversationRecord> {
        if let Some(mut record) = self.find_conversation(scope, alias).await? {
            if record.content.account != Some(account) {
                record.content.account = Some(account);
                self.put(record.to_object()?).await?;
            }
            return Ok(record);
        }
        self.create_conversation(account, scope, vec![alias.clone()], name, kind)
            .await
    }

    pub(super) async fn create_conversation(
        &self,
        account: ObjectId,
        scope: &str,
        aliases: Vec<ExternalId>,
        name: &str,
        kind: Kind,
    ) -> Result<ConversationRecord> {
        let shown = if name.trim().is_empty() {
            "Conversation"
        } else {
            name
        };
        let mut envelope = Envelope::new(CONVERSATION, shown);
        envelope.iri = self.iri(envelope.id);
        let record = ConversationRecord {
            envelope,
            content: ConversationContent {
                account: Some(account),
                scope: scope.to_string(),
                kind,
                aliases,
                ..ConversationContent::default()
            },
        };
        self.put(record.to_object()?).await?;
        Ok(record)
    }

    /// Recompute what is derived from the messages. Writes only if something changed.
    pub(super) async fn refresh(&self, id: ObjectId) -> Result<()> {
        let record = self.load_conversation(id).await?;
        let context = id.to_string();
        let notes = || Query::new().of_type(NOTE).in_context(context.clone());
        let unread = self
            .store
            .count(&notes().with_index(IDX_UNREAD, YES))
            .await?;
        let last = self.first_readable(notes(), IDX_DESC).await?;
        let mut new = record.clone();
        new.content.unread_count = u32::try_from(unread).unwrap_or(u32::MAX);
        new.content.last_message = None;
        new.content.last_at = None;
        new.content.encryption = Encryption::Unknown;
        if let Some(m) = last {
            new.content.last_message = Some(m.id());
            new.content.last_at = Some(m.sort_time());
            new.content.encryption = m.summary.encryption;
        }
        if new != record {
            self.put(new.to_object()?).await?;
        }
        Ok(())
    }

    async fn summary_of(&self, record: ConversationRecord) -> Result<ConversationSummary> {
        let c = record.content;
        // Gone or unreadable is no reason to lose the conversation.
        let last_message = match c.last_message {
            Some(id) => match self.load_note(id).await {
                Ok(m) => Some(m.summary),
                Err(Error::NotFound(_) | Error::Json(_)) => None,
                Err(e) => return Err(e),
            },
            None => None,
        };
        Ok(ConversationSummary {
            id: record.envelope.id,
            name: record.envelope.name,
            kind: c.kind,
            encryption: c.encryption,
            unread_count: c.unread_count,
            last_message,
            pinned: c.pinned,
            folders: c.folders,
        })
    }

    pub(super) async fn open_conversation(&self, id: ObjectId) -> Result<ConversationSummary> {
        let record = self.load_conversation(id).await?;
        self.summary_of(record).await
    }

    pub(super) async fn list_conversations(
        &self,
        folder: Option<&str>,
        only_unread: bool,
        cursor: Option<&Cursor>,
    ) -> Result<ConversationPage> {
        let size = self.config.page_size;
        let mut q = Query::new()
            .of_type(CONVERSATION)
            .order_by_index(IDX_SORT)
            .limit(size + 1);
        if let Some(f) = folder {
            q = q.with_index(IDX_FOLDER, f);
        }
        if only_unread {
            q = q.with_index(IDX_UNREAD, YES);
        }
        if let Some(c) = cursor {
            q = q.index_op(IDX_SORT, Op::Gt, c.as_str());
        }
        let (objects, next) = split_page(self.store.query(&q).await?, size, IDX_SORT);
        let mut page = Page {
            items: Vec::with_capacity(objects.len()),
            next,
            skipped: vec![],
        };
        for o in objects {
            let id = o.envelope.id;
            match ConversationRecord::from_object(o) {
                Ok(record) => page.items.push(self.summary_of(record).await?),
                Err(_) => page.skipped.push(id),
            }
        }
        Ok(page)
    }
}
