//! [`JmapSubmit`]: the JMAP strategy for sending (RFC 8621 section 7).
//!
//! Pattern: Strategy. The finished bytes are uploaded, imported into the sent mailbox and submitted in
//! one request, with an explicit envelope for the hidden recipients, since no header names them. So the
//! sent copy is the very mail that left. If the submission is refused the copy is destroyed, so no
//! mail claims to be sent that was not.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::client::{JmapClient, err};
use crate::error::{MailError, Result};
use crate::submit::{MailEnvelope, MailSubmit, SentCopy};

const MAIL: &str = "urn:ietf:params:jmap:mail";
const SUBMISSION: &str = "urn:ietf:params:jmap:submission";

/// Submits over JMAP. The server keeps the sent copy.
pub struct JmapSubmit {
    client: Arc<JmapClient>,
}

impl JmapSubmit {
    /// A strategy over `client`.
    pub fn new(client: Arc<JmapClient>) -> Self {
        Self { client }
    }

    /// The sent mailbox and the identity of `from`.
    async fn prepare(&self, account: &str, from: &str) -> Result<(String, String)> {
        let call = json!([
            ["Mailbox/get", {"accountId": account, "ids": null, "properties": ["id", "role"]}, "m"],
            ["Identity/get", {"accountId": account, "ids": null}, "i"],
        ]);
        let answers = self.client.call(&[MAIL, SUBMISSION], call).await?;
        let sent = JmapClient::answer(&answers, "m")?["list"]
            .as_array()
            .and_then(|l| l.iter().find(|m| m["role"] == "sent"))
            .and_then(|m| m["id"].as_str())
            .ok_or_else(|| err("the account has no mailbox for sent mail"))?
            .to_string();
        let identities = JmapClient::answer(&answers, "i")?["list"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let identity = identities
            .iter()
            .find(|i| {
                i["email"]
                    .as_str()
                    .is_some_and(|e| e.eq_ignore_ascii_case(from))
            })
            .and_then(|i| i["id"].as_str())
            .ok_or_else(|| err(format!("no identity for {from}")))?
            .to_string();
        Ok((sent, identity))
    }

    async fn destroy(&self, account: &str, email: &str) {
        let call = json!([["Email/set", {"accountId": account, "destroy": [email]}, "d"]]);
        if let Err(e) = self.client.call(&[MAIL], call).await {
            tracing::warn!(error = %e, "could not remove the copy of a mail that did not leave");
        }
    }
}

fn created_id<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args["created"][key]["id"].as_str()
}

#[async_trait]
impl MailSubmit for JmapSubmit {
    async fn submit(&self, envelope: &MailEnvelope, raw: &[u8]) -> Result<SentCopy, MailError> {
        let account = self.client.session().await?.account.clone();
        let (sent, identity) = self.prepare(&account, &envelope.from).await?;
        let blob = self.client.upload(raw.to_vec()).await?;
        let rcpt: Vec<Value> = envelope
            .recipients
            .iter()
            .map(|r| json!({"email": r}))
            .collect();
        let call = json!([
            ["Email/import", {"accountId": account, "emails": {
                "mail": {"blobId": blob, "mailboxIds": {sent: true}, "keywords": {"$seen": true}}}}, "i"],
            ["EmailSubmission/set", {"accountId": account, "create": {"go": {
                "identityId": identity, "emailId": "#mail",
                "envelope": {"mailFrom": {"email": envelope.from}, "rcptTo": rcpt}}}}, "s"],
        ]);
        let answers = self.client.call(&[MAIL, SUBMISSION], call).await?;
        let import = JmapClient::answer(&answers, "i")?;
        let Some(email) = created_id(import, "mail") else {
            return Err(err(format!(
                "Email/import refused: {}",
                import["notCreated"]
            )));
        };
        let submission = JmapClient::answer(&answers, "s");
        match submission.map(|args| (created_id(args, "go").is_some(), args["notCreated"].clone()))
        {
            Ok((true, _)) => Ok(SentCopy::Stored),
            Ok((false, why)) => {
                self.destroy(&account, email).await;
                Err(err(format!("EmailSubmission/set refused: {why}")))
            }
            Err(e) => {
                self.destroy(&account, email).await;
                Err(e)
            }
        }
    }
}
