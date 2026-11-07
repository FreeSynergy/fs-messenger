//! Search: substring over subject and text, newest first.
//!
//! Pattern: Builder: `SearchFilter` becomes one store `Query`. Tokenised full text is a gap of
//! `fs-objects`, see `DESIGN.md`.

use fs_objects::{Cond, Op, Query, TimeField};

use super::consts::{IDX_DESC, IDX_FROM, IDX_HAS_ATTACHMENT, IDX_TEXT, IDX_UNREAD, NOTE, YES};
use super::handle::Inner;
use crate::error::Result;
use crate::model::{Cursor, MessagePage, SearchFilter};

impl Inner {
    pub(super) async fn search(
        &self,
        text: &str,
        filter: &SearchFilter,
        cursor: Option<&Cursor>,
    ) -> Result<MessagePage> {
        let size = self.config.page_size;
        let mut q = Query::new()
            .of_type(NOTE)
            .order_by_index(IDX_DESC)
            .limit(size + 1);
        if !text.is_empty() {
            q = q
                .or(Cond::Name {
                    op: Op::Contains,
                    value: text.to_string(),
                })
                .or(Cond::Index {
                    key: IDX_TEXT.to_string(),
                    op: Op::Contains,
                    value: text.to_string(),
                });
        }
        if let Some(from) = &filter.from {
            q = q.index_op(IDX_FROM, Op::Contains, from.clone());
        }
        if filter.unread {
            q = q.with_index(IDX_UNREAD, YES);
        }
        if filter.has_attachment {
            q = q.with_index(IDX_HAS_ATTACHMENT, YES);
        }
        if let Some(since) = filter.since {
            q = q.time(TimeField::Published, Op::Ge, since);
        }
        if let Some(c) = cursor {
            q = q.index_op(IDX_DESC, Op::Gt, c.as_str());
        }
        self.page_of(q, size).await
    }
}
