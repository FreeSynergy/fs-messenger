//! [`JmapFetch`]: the JMAP strategy for reading.
//!
//! Pattern: Strategy + State per mailbox: first a listing (`Email/query`), then changes (`Email/changes`).
//! The `Email` state is read BEFORE the listing starts, so whatever arrives meanwhile comes again through
//! the changes, and the core dedupes it. Provisional: a flag change on an old mail lists it as updated
//! and downloads it once more; costs bandwidth, stores nothing.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::client::{JmapClient, err};
use crate::error::{MailError, Result};
use crate::fetch::{FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole};

const MAIL: &str = "urn:ietf:params:jmap:mail";

/// Reads mail over JMAP.
pub struct JmapFetch {
    client: Arc<JmapClient>,
}

struct Mailboxes {
    inbox: String,
    sent: Option<String>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

impl JmapFetch {
    /// A strategy over `client`.
    pub fn new(client: Arc<JmapClient>) -> Self {
        Self { client }
    }

    async fn account(&self) -> Result<String> {
        Ok(self.client.session().await?.account.clone())
    }

    async fn mailboxes(&self) -> Result<Mailboxes> {
        let call = json!([["Mailbox/get", {"accountId": self.account().await?, "ids": null,
            "properties": ["id", "role"]}, "m"]]);
        let answers = self.client.call(&[MAIL], call).await?;
        let list = &JmapClient::answer(&answers, "m")?["list"];
        let by_role = |role: &str| {
            list.as_array()?.iter().find(|m| m["role"] == role)?["id"]
                .as_str()
                .map(str::to_string)
        };
        Ok(Mailboxes {
            inbox: by_role("inbox").ok_or_else(|| err("the account has no inbox"))?,
            sent: by_role("sent"),
        })
    }

    /// Ids of a mailbox's next page in the listing, and the cursor after it.
    async fn listing(
        &self,
        mailbox: &str,
        state: Option<String>,
        position: u64,
        limit: usize,
    ) -> Result<(Vec<String>, MailboxCursor)> {
        let account = self.account().await?;
        let call = json!([
            ["Email/get", {"accountId": account, "ids": [], "properties": ["id"]}, "s"],
            ["Email/query", {"accountId": account, "filter": {"inMailbox": mailbox},
                "sort": [{"property": "receivedAt", "isAscending": true}],
                "position": position, "limit": limit}, "q"],
        ]);
        let answers = self.client.call(&[MAIL], call).await?;
        let state = match state {
            Some(s) => s,
            None => JmapClient::answer(&answers, "s")?["state"]
                .as_str()
                .ok_or_else(|| err("Email/get answered no state"))?
                .to_string(),
        };
        let ids = strings(&JmapClient::answer(&answers, "q")?["ids"]);
        let cursor = if ids.len() < limit {
            MailboxCursor::Jmap {
                state,
                position: None,
            }
        } else {
            MailboxCursor::Jmap {
                state,
                position: Some(position + ids.len() as u64),
            }
        };
        Ok((ids, cursor))
    }

    /// What changed since `state`. `None` if the server cannot say any more: start over.
    async fn changes(
        &self,
        state: &str,
        limit: usize,
    ) -> Result<Option<(Vec<String>, MailboxCursor, bool)>> {
        let call = json!([["Email/changes", {"accountId": self.account().await?,
            "sinceState": state, "maxChanges": limit}, "c"]]);
        let answers = self.client.call(&[MAIL], call).await?;
        let args = match JmapClient::response(&answers, "c")? {
            Ok(args) => args,
            Err(e) if e.kind == "cannotCalculateChanges" => return Ok(None),
            Err(e) => return Err(err(format!("Email/changes answered {}", e.kind))),
        };
        let mut ids = strings(&args["created"]);
        ids.extend(strings(&args["updated"]));
        let next = args["newState"]
            .as_str()
            .ok_or_else(|| err("Email/changes answered no newState"))?
            .to_string();
        let cursor = MailboxCursor::Jmap {
            state: next,
            position: None,
        };
        Ok(Some((ids, cursor, args["hasMoreChanges"] == true)))
    }

    /// Download those of `ids` that are in `mailbox`. Ids that vanished meanwhile are simply gone.
    async fn download(
        &self,
        ids: Vec<String>,
        mailbox: &str,
        role: MailboxRole,
        out: &mut Vec<FetchedMail>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let call = json!([["Email/get", {"accountId": self.account().await?, "ids": ids,
            "properties": ["id", "blobId", "mailboxIds"]}, "g"]]);
        let answers = self.client.call(&[MAIL], call).await?;
        let list = JmapClient::answer(&answers, "g")?["list"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for email in list.iter().filter(|e| e["mailboxIds"][mailbox] == true) {
            let (Some(id), Some(blob)) = (email["id"].as_str(), email["blobId"].as_str()) else {
                return Err(err("Email/get answered a mail without id or blobId"));
            };
            out.push(FetchedMail {
                bytes: self.client.download(blob).await?,
                role,
                stable_id: format!("{mailbox}/{id}"),
            });
        }
        Ok(())
    }

    async fn read_mailbox(
        &self,
        mailbox: &str,
        role: MailboxRole,
        limit: usize,
        batch: &mut FetchedBatch,
    ) -> Result<()> {
        let held = batch.cursor.mailboxes.get(mailbox).cloned();
        let (state, position) = match held {
            Some(MailboxCursor::Jmap { state, position }) => (Some(state), position),
            _ => (None, None),
        };
        let changes = match (&state, position) {
            (Some(state), None) => self.changes(state, limit).await?,
            _ => None,
        };
        let (ids, next, more) = if let Some(answer) = changes {
            answer
        } else {
            // A first listing, its next page, or the server can no longer say what changed.
            let state = state.filter(|_| position.is_some());
            let (ids, next) = self
                .listing(mailbox, state, position.unwrap_or(0), limit)
                .await?;
            (ids, next, true)
        };
        self.download(ids, mailbox, role, &mut batch.mails).await?;
        batch.more |= more;
        batch.cursor.mailboxes.insert(mailbox.to_string(), next);
        Ok(())
    }
}

#[async_trait]
impl MailFetch for JmapFetch {
    async fn fetch(&self, cursor: &MailCursor, limit: usize) -> Result<FetchedBatch, MailError> {
        let boxes = self.mailboxes().await?;
        let mut batch = FetchedBatch {
            cursor: cursor.clone(),
            ..FetchedBatch::default()
        };
        self.read_mailbox(&boxes.inbox, MailboxRole::Inbox, limit, &mut batch)
            .await?;
        if let Some(sent) = &boxes.sent {
            self.read_mailbox(sent, MailboxRole::Sent, limit, &mut batch)
                .await?;
        }
        Ok(batch)
    }

    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError> {
        let boxes = self.mailboxes().await?;
        let sent = boxes
            .sent
            .ok_or_else(|| err("the account has no mailbox for sent mail"))?;
        let blob = self.client.upload(raw.to_vec()).await?;
        let call = json!([["Email/import", {"accountId": self.account().await?, "emails": {
            "copy": {"blobId": blob, "mailboxIds": {sent: true}, "keywords": {"$seen": true}}}}, "i"]]);
        let answers = self.client.call(&[MAIL], call).await?;
        let args = JmapClient::answer(&answers, "i")?;
        if args["created"]["copy"].is_null() {
            return Err(err(format!("Email/import refused: {}", args["notCreated"])));
        }
        Ok(())
    }
}
