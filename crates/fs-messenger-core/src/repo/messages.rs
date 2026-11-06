//! Reading messages and marking them read.
//!
//! Pattern: Repository, with cursor paging over the fixed-width order keys. "Read up to X" follows
//! the message order, not the arrival order, so a late older message does not stay unread.

use fs_store::{ActivityKind, LogEntry, ObjectId, Op, Query};

use super::consts::{IDX_DESC, IDX_ORDER, IDX_UNREAD, NOTE, TOMBSTONE, YES};
use super::handle::Inner;
use super::keys::order_key;
use super::message_codec::{message_from, message_to};
use super::paging::split_page;
use crate::error::{Error, Result};
use crate::model::{Cursor, Message, MessagePage, MessageSummary, Page};

impl Inner {
    pub(super) async fn load_note(&self, id: ObjectId) -> Result<Message> {
        let o = self
            .store
            .get(id)
            .await?
            .filter(|o| o.envelope.object_type == NOTE)
            .ok_or(Error::NotFound(id))?;
        message_from(o)
    }

    /// A message only counts in its own conversation.
    pub(super) async fn load_in(&self, conversation: ObjectId, id: ObjectId) -> Result<Message> {
        let m = self.load_note(id).await?;
        if m.summary.envelope.context.as_deref() != Some(conversation.to_string().as_str()) {
            return Err(Error::NotFound(id));
        }
        Ok(m)
    }

    pub(super) async fn messages_page(
        &self,
        conversation: ObjectId,
        cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        self.load_conversation(conversation).await?;
        let size = self.config.page_size;
        let mut q = Query::new()
            .of_type(NOTE)
            .in_context(conversation.to_string())
            .order_by_index(IDX_DESC)
            .limit(size + 1);
        if let Some(c) = cursor {
            q = q.index_op(IDX_DESC, Op::Gt, c.as_str());
        }
        self.page_of(q, size).await
    }

    /// Run a newest-first query that asks for one more than `size`.
    pub(super) async fn page_of(&self, q: Query, size: usize) -> Result<MessagePage> {
        let (objects, next) = split_page(self.store.query(&q).await?, size, IDX_DESC);
        let items = objects
            .into_iter()
            .map(|o| Ok(message_from(o)?.summary))
            .collect::<Result<Vec<_>>>()?;
        Ok(Page { items, next })
    }

    pub(super) async fn mark_read(&self, conversation: ObjectId, up_to: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let target = self.load_in(conversation, up_to).await?;
        let key = order_key(target.summary.envelope.published, up_to);
        let q = Query::new()
            .of_type(NOTE)
            .in_context(conversation.to_string())
            .with_index(IDX_UNREAD, YES)
            .index_op(IDX_ORDER, Op::Le, key);
        for o in self.store.query(&q).await? {
            let mut m = message_from(o)?;
            m.summary.unread = false;
            let read = vec![LogEntry::new(ActivityKind::Read, serde_json::json!({}))];
            self.store
                .put_and_log(message_to(&m)?, &self.config.actor, read)
                .await?;
        }
        self.refresh(conversation).await
    }

    /// The oldest unread message after `after`. A deleted `after` still counts: its tombstone keeps
    /// the place, because swiping a message away is how a frontend gets here.
    pub(super) async fn next_unread(
        &self,
        after: Option<ObjectId>,
    ) -> Result<Option<MessageSummary>> {
        let mut q = Query::new()
            .of_type(NOTE)
            .with_index(IDX_UNREAD, YES)
            .order_by_index(IDX_ORDER)
            .limit(1);
        if let Some(id) = after {
            let o = self
                .store
                .get(id)
                .await?
                .filter(|o| [NOTE, TOMBSTONE].contains(&o.envelope.object_type.as_str()))
                .ok_or(Error::NotFound(id))?;
            let key = o.index.get(IDX_ORDER).and_then(|v| v.first()).cloned();
            let key = key.ok_or_else(|| Error::Invalid(format!("{id} has no order")))?;
            q = q.index_op(IDX_ORDER, Op::Gt, key);
        }
        match self.store.query(&q).await?.pop() {
            Some(o) => Ok(Some(message_from(o)?.summary)),
            None => Ok(None),
        }
    }
}
