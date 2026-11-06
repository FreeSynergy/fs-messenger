//! `act`: what a person does to one message.
//!
//! Pattern: Command: `MessageAction` is the operation, `apply` the executor. Edit, delete, react and
//! read stay local in this version: `Provider` has no method to carry them, see `DESIGN.md`.

use fs_interfaces::provider::Unsupported;
use fs_store::{ActivityKind, LogEntry, ObjectId};

use super::handle::Inner;
use super::message_codec::message_to;
use super::text::{display_name, preview};
use crate::error::{Error, Result};
use crate::model::{Message, MessageAction, Reaction};

/// One own reaction per message: a new key replaces the old one, empty takes it back.
fn react(m: &mut Message, key: &str) {
    for r in &mut m.reactions {
        if r.mine {
            r.mine = false;
            r.count = r.count.saturating_sub(1);
        }
    }
    if !key.is_empty() {
        if let Some(r) = m.reactions.iter_mut().find(|r| r.key == key) {
            r.mine = true;
            r.count += 1;
        } else {
            m.reactions.push(Reaction {
                key: key.to_string(),
                count: 1,
                mine: true,
            });
        }
    }
    m.reactions.retain(|r| r.count > 0);
}

fn edit(m: &mut Message, text: String) -> Result<()> {
    if !m.outgoing {
        return Err(Error::Conflict("only my own messages can be edited".into()));
    }
    if text.trim().is_empty() {
        return Err(Error::Invalid("an edit needs text; delete instead".into()));
    }
    m.summary.preview = preview(&text);
    m.summary.envelope.name = display_name(None, &text);
    m.body_text = text;
    Ok(())
}

impl Inner {
    pub(super) async fn act(
        &self,
        conversation: ObjectId,
        id: ObjectId,
        action: MessageAction,
    ) -> Result<()> {
        match action {
            MessageAction::None => Err(Error::Invalid("no action given; nothing was done".into())),
            MessageAction::Decide(_) => Err(Unsupported::new(
                "toolcall.decide",
                "the guard that checks a person's decision is not built yet",
            )
            .into()),
            MessageAction::Delete => self.delete_message(conversation, id).await,
            MessageAction::Forward(recipients) => self.forward(conversation, id, recipients).await,
            MessageAction::Flag(on) => {
                self.change(conversation, id, None, |m| {
                    m.summary.flagged = on;
                    Ok(())
                })
                .await
            }
            MessageAction::Unread(on) => {
                self.change(conversation, id, None, |m| {
                    m.summary.unread = on;
                    Ok(())
                })
                .await
            }
            MessageAction::React(key) => {
                let like = (!key.is_empty())
                    .then(|| LogEntry::new(ActivityKind::Like, serde_json::json!({ "key": key })));
                self.change(conversation, id, like, |m| {
                    react(m, &key);
                    Ok(())
                })
                .await
            }
            MessageAction::Edit(text) => {
                self.change(conversation, id, None, |m| edit(m, text)).await
            }
        }
    }

    /// Load, change, store under the same id, and bring the conversation up to date.
    async fn change(
        &self,
        conversation: ObjectId,
        id: ObjectId,
        extra: Option<LogEntry>,
        change: impl FnOnce(&mut Message) -> Result<()> + Send,
    ) -> Result<()> {
        let _writes = self.writes.lock().await;
        let mut m = self.load_in(conversation, id).await?;
        change(&mut m)?;
        let extra = extra.into_iter().collect();
        self.store
            .put_and_log(message_to(&m)?, &self.config.actor, extra)
            .await?;
        self.refresh(conversation).await
    }
}
