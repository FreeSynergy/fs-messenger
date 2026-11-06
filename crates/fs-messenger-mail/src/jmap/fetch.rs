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
    Budget, FetchLimits, FetchedBatch, FetchedMail, HEAD_BYTES, MailCursor, MailFetch,
    MailboxCursor, MailboxRole, sent_is_optional,
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
            pending: vec![],
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
            pending: vec![],
        };
        Ok(Some((ids, cursor, args["hasMoreChanges"] == true)))
    }

    /// Download those of `ids` that are in `mailbox`, as far as the budget goes, and answer the ids left
    /// for later. Ids that vanished meanwhile are simply gone.
    async fn download(
        &self,
        ids: Vec<String>,
        (mailbox, role): (&str, MailboxRole),
        limits: &FetchLimits,
        budget: &mut Budget,
        out: &mut Vec<FetchedMail>,
    ) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let call = json!([["Email/get", {"accountId": self.account().await?, "ids": ids,
            "properties": ["id", "blobId", "mailboxIds", "size"]}, "g"]]);
        let answers = self.client.call(&[MAIL], call).await?;
        let list = JmapClient::answer(&answers, "g")?["list"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let bytes = u64::try_from(limits.bytes).unwrap_or(u64::MAX);
        let mut left = vec![];
        for email in list.iter().filter(|e| e["mailboxIds"][mailbox] == true) {
            let (Some(id), Some(blob)) = (email["id"].as_str(), email["blobId"].as_str()) else {
                return Err(err("Email/get answered a mail without id or blobId"));
            };
            let size = email["size"].as_u64().unwrap_or(0);
            let over = size > bytes;
            if !left.is_empty() || !budget.admit(if over { HEAD_BYTES } else { size }) {
                left.push(id.to_string());
                continue;
            }
            let keep = if over { HEAD_BYTES } else { bytes };
            let (body, cut) = self.client.download(blob, keep).await?;
            out.push(FetchedMail {
                bytes: body,
                role,
                stable_id: format!("{mailbox}/{id}"),
                // A server that said small and sends more is cut off at the limit as well.
                too_large: (over || cut).then_some(size.max(keep)),
            });
        }
        Ok(left)
    }

    async fn read_mailbox(
        &self,
        (mailbox, role): (&str, MailboxRole),
        limits: &FetchLimits,
        budget: &mut Budget,
        batch: &mut FetchedBatch,
    ) -> Result<()> {
        let held = batch.cursor.mailboxes.get(mailbox).cloned();
        let (ids, mut next, mut more) = match held {
            Some(MailboxCursor::Jmap {
                state,
                walking,
                anchor,
                pending,
            }) if !pending.is_empty() => {
                // First what the last call had to leave; the walk or the changes go on after that.
                let cursor = MailboxCursor::Jmap {
                    state,
                    walking,
                    anchor,
                    pending: vec![],
                };
                (pending, cursor, true)
            }
            Some(MailboxCursor::Jmap {
                state,
                walking: false,
                ..
            }) => match self.changes(&state, limits.mails).await? {
                Some(answer) => answer,
                None => self.first_page(mailbox, None, None, limits).await?,
            },
            Some(MailboxCursor::Jmap { state, anchor, .. }) => {
                self.first_page(mailbox, Some(state), anchor, limits)
                    .await?
            }
            _ => self.first_page(mailbox, None, None, limits).await?,
        };
        let left = self
            .download(ids, (mailbox, role), limits, budget, &mut batch.mails)
            .await?;
        if !left.is_empty() {
            more = true;
            if let MailboxCursor::Jmap { pending, .. } = &mut next {
                *pending = left;
            }
        }
        batch.more |= more;
        batch.cursor.mailboxes.insert(mailbox.to_string(), next);
        Ok(())
    }

    /// A first listing, its next page, or the server can no longer say what changed.
    async fn first_page(
        &self,
        mailbox: &str,
        state: Option<String>,
        anchor: Option<String>,
        limits: &FetchLimits,
    ) -> Result<(Vec<String>, MailboxCursor, bool)> {
        let (ids, next) = self.listing(mailbox, state, anchor, limits.mails).await?;
        Ok((ids, next, true))
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
        let mut budget = Budget::new(limits.batch_bytes);
        self.read_mailbox(
            (&boxes.inbox, MailboxRole::Inbox),
            limits,
            &mut budget,
            &mut batch,
        )
        .await?;
        if let Some(sent) = &boxes.sent {
            sent_is_optional(
                self.read_mailbox((sent, MailboxRole::Sent), limits, &mut budget, &mut batch)
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
