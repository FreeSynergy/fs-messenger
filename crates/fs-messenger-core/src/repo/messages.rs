//! Reading messages and marking them read.
//!
//! Pattern: Repository, with cursor paging over fixed-width keys. "Read up to X" and "next unread"
//! run over the local arrival order, not over a date a sender chose: an old date cannot be swept
//! along by a later mark, and a date far ahead cannot sweep up what arrived after it.

use fs_store::{ActivityKind, LogEntry, ObjectId, Op, Query};

use super::consts::{IDX_ARRIVAL, IDX_DESC, IDX_UNREAD, NOTE, TOMBSTONE, YES};
use super::handle::Inner;
use super::keys::{arrival_key, desc_key};
use super::message_codec::{message_from, message_to};
use super::paging::{decoded, split_page};
use crate::error::{Error, Result};
use crate::model::{Cursor, Message, MessagePage, MessageSummary};

/// How many records one step looks at when it has to walk past unreadable ones.
const WALK: usize = 16;

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

    /// A whole message to read, its text included.
    pub(super) async fn read_message(
        &self,
        conversation: ObjectId,
        id: ObjectId,
    ) -> Result<Message> {
        let m = self.load_in(conversation, id).await?;
        self.hydrate(m).await
    }

    /// The first message in the order of `key` that can be read.
    pub(super) async fn first_readable(&self, base: Query, key: &str) -> Result<Option<Message>> {
        let first = base.clone().order_by_index(key).limit(WALK);
        let mut q = first;
        loop {
            let objects = self.store.query(&q).await?;
            let full = objects.len() == WALK;
            let last = objects
                .last()
                .and_then(|o| o.index.get(key))
                .and_then(|v| v.first())
                .cloned();
            if let Some(m) = objects.into_iter().find_map(|o| message_from(o).ok()) {
                return Ok(Some(m));
            }
            let Some(last) = last.filter(|_| full) else {
                return Ok(None);
            };
            q = base
                .clone()
                .order_by_index(key)
                .limit(WALK)
                .index_op(key, Op::Gt, last);
        }
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
        Ok(decoded(objects, next, |o| Ok(message_from(o)?.summary)))
    }

    /// Swept only what is both at or before `up_to` in arrival and at or below it on screen. A later
    /// arrival never goes along, whatever its date, and reading a backdated mail does not take the
    /// newer mail above it along. A record that cannot be read stays as it is.
    pub(super) async fn mark_read(&self, conversation: ObjectId, up_to: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let target = self.load_in(conversation, up_to).await?;
        let q = Query::new()
            .of_type(NOTE)
            .in_context(conversation.to_string())
            .with_index(IDX_UNREAD, YES)
            .index_op(
                IDX_ARRIVAL,
                Op::Le,
                arrival_key(target.summary.arrival, up_to),
            )
            .index_op(
                IDX_DESC,
                Op::Ge,
                desc_key(target.sort_time(), target.summary.arrival, up_to),
            );
        for o in self.store.query(&q).await? {
            let Ok(mut m) = message_from(o) else {
                continue;
            };
            m.summary.unread = false;
            let read = vec![LogEntry::new(ActivityKind::Read, serde_json::json!({}))];
            self.store
                .put_and_log(message_to(&m)?, &self.config.actor, read)
                .await?;
        }
        self.refresh(conversation).await
    }

    /// The first arrived unread message after `after`. A deleted `after` still counts: its tombstone
    /// keeps the place, because swiping a message away is how a frontend gets here.
    pub(super) async fn next_unread(
        &self,
        after: Option<ObjectId>,
    ) -> Result<Option<MessageSummary>> {
        let mut q = Query::new().of_type(NOTE).with_index(IDX_UNREAD, YES);
        if let Some(id) = after {
            let o = self
                .store
                .get(id)
                .await?
                .filter(|o| [NOTE, TOMBSTONE].contains(&o.envelope.object_type.as_str()))
                .ok_or(Error::NotFound(id))?;
            let key = o.index.get(IDX_ARRIVAL).and_then(|v| v.first()).cloned();
            let key = key.ok_or_else(|| Error::Invalid(format!("{id} has no arrival")))?;
            q = q.index_op(IDX_ARRIVAL, Op::Gt, key);
        }
        Ok(self
            .first_readable(q, IDX_ARRIVAL)
            .await?
            .map(|m| m.summary))
    }
}
