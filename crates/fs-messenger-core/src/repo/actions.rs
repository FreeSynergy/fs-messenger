//! `act`: what a person does to one message.
//!
//! Pattern: Command: `MessageAction` is the operation, `apply` the executor. Flag and read state are
//! local by nature. Edit and react would have to reach the other side and `Provider` has no method to
//! carry them, so they answer `Unsupported` instead of claiming a success that never left this device.

use fs_interfaces::provider::Unsupported;
use fs_store::ObjectId;

use super::handle::Inner;
use super::message_codec::message_to;
use crate::error::{Error, Result};
use crate::model::{Message, MessageAction};

const NOT_CARRIED: &str = "no provider can carry this to the other side yet";

impl Inner {
    pub(super) async fn act(
        &self,
        conversation: ObjectId,
        id: ObjectId,
        action: MessageAction,
    ) -> Result<()> {
        match action {
            MessageAction::None => Err(Error::Invalid("no action given; nothing was done".into())),
            MessageAction::Decide(decision) => match self.guard() {
                Some(guard) => guard.decide(conversation, id, decision).await,
                None => Err(Unsupported::new("toolcall.decide", "no guard is installed").into()),
            },
            MessageAction::Edit(_) => Err(Unsupported::new("message.edit", NOT_CARRIED).into()),
            MessageAction::React(_) => Err(Unsupported::new("message.react", NOT_CARRIED).into()),
            MessageAction::Delete => self.delete_message(conversation, id).await,
            MessageAction::Forward(recipients) => self.forward(conversation, id, recipients).await,
            MessageAction::Flag(on) => {
                self.change(conversation, id, |m| m.summary.flagged = on)
                    .await
            }
            MessageAction::Unread(on) => {
                self.change(conversation, id, |m| m.summary.unread = on)
                    .await
            }
        }
    }

    /// Load, change, store under the same id, and bring the conversation up to date.
    async fn change(
        &self,
        conversation: ObjectId,
        id: ObjectId,
        change: impl FnOnce(&mut Message) + Send,
    ) -> Result<()> {
        let _writes = self.writes.lock().await;
        let mut m = self.load_in(conversation, id).await?;
        change(&mut m);
        self.put(message_to(&m)?).await?;
        self.refresh(conversation).await
    }
}
