//! What a JMAP mailbox has: its listing and its changes.
//!
//! Pattern: Repository (the reading queries of [`JmapFetch`]). The listing pages by `anchor`: a position
//! would skip a mail when an earlier one is deleted between two pages. If the anchor itself is gone, the
//! walk starts over.

use serde_json::{Value, json};

use super::client::{JmapClient, err};
use super::fetch::{JmapFetch, MAIL};
use crate::error::Result;
use crate::fetch::MailboxCursor;

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
    /// Ids of a mailbox's next page in the listing, and the cursor after it.
    pub(super) async fn listing(
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
    pub(super) async fn changes(
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
}
