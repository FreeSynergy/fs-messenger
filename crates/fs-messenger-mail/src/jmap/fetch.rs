//! [`JmapFetch`]: the JMAP strategy for reading.
//!
//! Pattern: Strategy + State per mailbox: first a listing (`Email/query`), then changes (`Email/changes`).
//! The `Email` state is read BEFORE the listing starts, so whatever arrives meanwhile comes again through
//! the changes, and the core dedupes it. The listing pages by `anchor`: a position would skip a mail when
//! an earlier one is deleted between two pages. If the anchor itself is gone, the walk starts over. Provisional: a flag change on an old mail lists it as updated
//! and downloads it once more; costs bandwidth, stores nothing.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::client::{JmapClient, err};
use crate::error::{MailError, Result};
use crate::fetch::{
    FetchLimits, FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole,
    sent_is_optional,
};

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
        anchor: Option<String>,
        limit: usize,
    ) -> Result<(Vec<String>, MailboxCursor)> {
        let account = self.account().await?;
        let mut query = json!({"accountId": account, "filter": {"inMailbox": mailbox},
            "sort": [{"property": "receivedAt", "isAscending": true}], "limit": limit});
        if let Some(anchor) = &anchor {
            query["anchor"] = json!(anchor);
            query["anchorOffset"] = json!(1);
        }
        let call = json!([
            ["Email/get", {"accountId": account, "ids": [], "properties": ["id"]}, "s"],
            ["Email/query", query, "q"],
        ]);
        let answers = self.client.call(&[MAIL], call).await?;
        let state = match state {
            Some(s) => s,
            None => JmapClient::answer(&answers, "s")?["state"]
                .as_str()
                .ok_or_else(|| err("Email/get answered no state"))?
                .to_string(),
        };
        let args = match JmapClient::response(&answers, "q")? {
            Ok(args) => args,
            Err(e) if e.kind == "anchorNotFound" && anchor.is_some() => {
                // The anchor was deleted. From the start again: the core dedupes what repeats, and the
                // state held is older than the walk, so nothing is lost.
                return Box::pin(self.listing(mailbox, Some(state), None, limit)).await;
            }
            Err(e) => return Err(err(format!("Email/query answered {}", e.kind))),
        };
        let ids = strings(&args["ids"]);
        let walking = ids.len() >= limit;
        let anchor = if walking { ids.last().cloned() } else { None };
        let cursor = MailboxCursor::Jmap {
            state,
            walking,
            anchor,
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
            walking: false,
            anchor: None,
        };
        Ok(Some((ids, cursor, args["hasMoreChanges"] == true)))
    }

    /// Download those of `ids` that are in `mailbox`. Ids that vanished meanwhile are simply gone.
    async fn download(
        &self,
        ids: Vec<String>,
        mailbox: &str,
        role: MailboxRole,
        limit: usize,
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
            let (bytes, truncated) = self.client.download(blob, limit).await?;
            out.push(FetchedMail {
                bytes,
                role,
                stable_id: format!("{mailbox}/{id}"),
                truncated,
            });
        }
        Ok(())
    }

    async fn read_mailbox(
        &self,
        mailbox: &str,
        role: MailboxRole,
        limits: &FetchLimits,
        batch: &mut FetchedBatch,
    ) -> Result<()> {
        let (state, walking, anchor) = match batch.cursor.mailboxes.get(mailbox).cloned() {
            Some(MailboxCursor::Jmap {
                state,
                walking,
                anchor,
            }) => (Some(state), walking, anchor),
            _ => (None, false, None),
        };
        let changes = match &state {
            Some(state) if !walking => self.changes(state, limits.mails).await?,
            _ => None,
        };
        let (ids, next, more) = if let Some(answer) = changes {
            answer
        } else {
            // A first listing, its next page, or the server can no longer say what changed.
            let state = state.filter(|_| walking);
            let (ids, next) = self.listing(mailbox, state, anchor, limits.mails).await?;
            (ids, next, true)
        };
        self.download(ids, mailbox, role, limits.bytes, &mut batch.mails)
            .await?;
        batch.more |= more;
        batch.cursor.mailboxes.insert(mailbox.to_string(), next);
        Ok(())
    }
}

#[async_trait]
impl MailFetch for JmapFetch {
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError> {
        let boxes = self.mailboxes().await?;
        let mut batch = FetchedBatch {
            cursor: cursor.clone(),
            ..FetchedBatch::default()
        };
        self.read_mailbox(&boxes.inbox, MailboxRole::Inbox, limits, &mut batch)
            .await?;
        if let Some(sent) = &boxes.sent {
            sent_is_optional(
                self.read_mailbox(sent, MailboxRole::Sent, limits, &mut batch)
                    .await,
            );
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
