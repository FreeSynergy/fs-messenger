//! Threading: who a message answers, in whatever order they arrive.
//!
//! Pattern: Repository over the alias fields. These are claims a sender makes: they can misplace a
//! reply, never lose a mail, and a link that would close a cycle is given up.

use fs_store::{Cond, ObjectId, Op, Query};

use super::consts::{IDX_ALIAS, IDX_ARRIVAL, IDX_LINK, IDX_RECEIPT, IDX_REPLY_ALIAS, NOTE};
use super::handle::Inner;
use super::keys::alias_key;
use super::message_codec::{message_from, message_to};
use crate::error::{Error, Result};
use crate::model::{ExternalId, Message};

const MAX_CHAIN: usize = 1000;

impl Inner {
    /// Our id of the parent if it is here, by anything it is known as; otherwise remember the claim.
    pub(super) async fn resolve_parent(
        &self,
        scope: &str,
        parent: Option<ExternalId>,
    ) -> Result<(Option<ObjectId>, Option<ExternalId>)> {
        let Some(parent) = parent else {
            return Ok((None, None));
        };
        // First arrived wins: a later message cannot take a thread over by claiming the same id.
        let mut q = Query::new().order_by_index(IDX_ARRIVAL).limit(1);
        for key in [IDX_ALIAS, IDX_LINK, IDX_RECEIPT] {
            q = q.or(Cond::Index {
                key: key.to_string(),
                op: Op::Eq,
                value: alias_key(scope, &parent),
            });
        }
        Ok(match self.store.query(&q).await?.pop() {
            Some(o) => (Some(o.envelope.id), None),
            None => (None, Some(parent)),
        })
    }

    /// Replies that arrived before their parent now learn its id; a re-sync can come in any order.
    /// A link that would close a cycle is given up: the reply stays in the conversation, unlinked.
    pub(super) async fn adopt_children(&self, scope: &str, parent: &Message) -> Result<()> {
        let known_as = parent
            .aliases
            .iter()
            .chain(&parent.links)
            .chain(&parent.receipts);
        for alias in known_as {
            let q = Query::new()
                .of_type(NOTE)
                .with_index(IDX_REPLY_ALIAS, alias_key(scope, alias));
            for o in self.store.query(&q).await? {
                // An unreadable record is store damage; it must not stop this message.
                let Ok(mut child) = message_from(o) else {
                    continue;
                };
                child.pending_reply = None;
                if !self.is_above(child.id(), parent).await? {
                    child.summary.envelope.in_reply_to = Some(parent.id().to_string());
                }
                self.put(message_to(&child)?).await?;
            }
        }
        Ok(())
    }

    /// Whether `ancestor` is `from` or above it in the reply chain. A chain longer than anyone
    /// writes counts as yes.
    async fn is_above(&self, ancestor: ObjectId, from: &Message) -> Result<bool> {
        let mut at = from.summary.envelope.in_reply_to.clone();
        for _ in 0..MAX_CHAIN {
            let Some(id) = at.and_then(|s| s.parse::<ObjectId>().ok()) else {
                return Ok(false);
            };
            if id == ancestor {
                return Ok(true);
            }
            let m = match self.load_note(id).await {
                Ok(m) => m,
                Err(Error::NotFound(_) | Error::Json(_)) => return Ok(false),
                Err(e) => return Err(e),
            };
            at = m.summary.envelope.in_reply_to;
        }
        Ok(true)
    }
}
