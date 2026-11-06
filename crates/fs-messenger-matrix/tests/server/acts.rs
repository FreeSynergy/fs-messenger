//! What a person does through the core and the provider.

use std::time::Duration;

use fs_messenger_core::{
    Ingest, Message, Messenger, Result as CoreResult, SearchFilter, SyncReport,
};
use fs_store::ObjectId;

use crate::common::Person;
use crate::http;

impl Person {
    /// One sync through the core.
    pub async fn sync(&self) -> SyncReport {
        self.repo.sync(self.account.id).await.expect("sync")
    }

    /// Send through the core's `Draft`.
    pub async fn send(&self, to: &[&str], text: &str) -> CoreResult<ObjectId> {
        let mut draft = self.repo.compose(None).await?;
        draft.set_account(self.account.id).await?;
        let to = to.iter().map(ToString::to_string).collect();
        draft.set_recipients(to, vec![], vec![]).await?;
        draft
            .set_text(text.to_owned(), "text/plain".to_owned())
            .await?;
        draft.send(None).await
    }

    /// Answer a message through the core's `Draft`.
    pub async fn reply(&self, to: ObjectId, text: &str) -> CoreResult<ObjectId> {
        let mut draft = self.repo.compose(Some(to)).await?;
        draft
            .set_text(text.to_owned(), "text/plain".to_owned())
            .await?;
        draft.send(None).await
    }

    /// All stored messages holding `text`.
    pub async fn find(&self, text: &str) -> Vec<Message> {
        let page = self
            .repo
            .search(text, &SearchFilter::default(), None)
            .await
            .unwrap();
        let mut found = Vec::new();
        for summary in page.items {
            let context = summary.envelope.context.as_deref().unwrap();
            let conversation: ObjectId = context.parse().unwrap();
            let id = summary.envelope.id;
            found.push(self.repo.message(conversation, id).await.unwrap());
        }
        found
    }

    /// Sync until `text` is here, or fail.
    pub async fn wait_for(&self, text: &str) -> Message {
        for _ in 0..80 {
            self.sync().await;
            if let Some(found) = self.find(text).await.pop() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("{} never got {text}", self.user_id);
    }

    /// Sync until invited, join, and sync the join in. Answers the room.
    pub async fn join_next_invite(&self) -> String {
        for _ in 0..80 {
            self.sync().await;
            if let Some(invite) = self.provider.invites().await.unwrap().pop() {
                self.provider.accept_invite(&invite.room_id).await.unwrap();
                self.sync().await;
                return invite.room_id;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("{} was never invited", self.user_id);
    }

    /// A token that reads what the server holds, from a device that has no keys.
    pub async fn server_view(&self) -> String {
        http::plain_login(&self.user_id, &self.password).await
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}
