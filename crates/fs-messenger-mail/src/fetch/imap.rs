//! [`ImapFetch`]: the IMAP strategy, with `async-imap` over rustls.
//!
//! Pattern: Strategy + Adapter. One connection per call, no pool; push through IDLE is `Unsupported`.
//! Mailboxes are opened read-only (`EXAMINE`, `BODY.PEEK`), so fetching marks nothing as read. Every call
//! runs under a timeout: a server that goes silent must not stall the account for ever.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_imap::Session;
use async_imap::imap_proto::NameAttribute;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::{
    FetchLimits, FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole,
    sent_is_optional,
};
use crate::error::{MailError, Result};
use crate::secret::Password;
use crate::tls::Endpoint;

const INBOX: &str = "INBOX";
/// Whole calls, up to 50 mails per mailbox: generous, and still an end.
pub const DEFAULT_IMAP_TIMEOUT: Duration = Duration::from_secs(300);

/// How to reach an IMAP server. Implicit TLS only.
#[derive(Clone, Debug)]
pub struct ImapConfig {
    /// The server.
    pub endpoint: Endpoint,
    /// Login name.
    pub user: String,
    /// Login secret.
    pub password: Password,
    /// How long one `fetch` or `store_sent` may take, connecting included. [`DEFAULT_IMAP_TIMEOUT`].
    pub timeout: Duration,
}

/// Reads mail over IMAP.
pub struct ImapFetch {
    config: ImapConfig,
}

fn err(e: impl std::fmt::Display) -> MailError {
    MailError::Imap(e.to_string())
}

type Stream = TlsStream<TcpStream>;

/// The `UID SEARCH` for what comes after `after`; nothing can come after the highest UID.
fn uids_after(after: u32) -> Option<String> {
    after.checked_add(1).map(|next| format!("UID {next}:*"))
}

impl ImapFetch {
    /// A strategy for `config`. Connects on use.
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    async fn session(&self) -> Result<Session<Stream>> {
        let endpoint = &self.config.endpoint;
        let tls = TlsConnector::from(Arc::new(endpoint.trust.client_config()?));
        let name = ServerName::try_from(endpoint.host.clone()).map_err(err)?;
        let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(err)?;
        let stream = tls.connect(name, tcp).await.map_err(err)?;
        let mut client = async_imap::Client::new(stream);
        client
            .read_response()
            .await
            .map_err(err)?
            .ok_or_else(|| err("the server closed before its greeting"))?;
        client
            .login(&self.config.user, self.config.password.expose())
            .await
            .map_err(|(e, _)| err(e))
    }

    /// The name of the mailbox flagged `\Sent` (RFC 6154), if the server names one.
    async fn sent_mailbox(session: &mut Session<Stream>) -> Result<Option<String>> {
        let names: Vec<_> = session
            .list(Some(""), Some("*"))
            .await
            .map_err(err)?
            .try_collect()
            .await
            .map_err(err)?;
        Ok(names
            .iter()
            .find(|n| {
                n.attributes().iter().any(|a| {
                    matches!(a, NameAttribute::Extension(x) if x.eq_ignore_ascii_case("\\Sent"))
                        || matches!(a, NameAttribute::Sent)
                })
            })
            .map(|n| n.name().to_string()))
    }

    /// The sizes of `uids`, by UID.
    async fn sizes(session: &mut Session<Stream>, set: &str) -> Result<HashMap<u32, u32>> {
        let fetched: Vec<_> = session
            .uid_fetch(set, "(UID RFC822.SIZE)")
            .await
            .map_err(err)?
            .try_collect()
            .await
            .map_err(err)?;
        Ok(fetched
            .iter()
            .filter_map(|f| Some((f.uid?, f.size?)))
            .collect())
    }

    async fn read_mailbox(
        session: &mut Session<Stream>,
        name: &str,
        role: MailboxRole,
        limits: &FetchLimits,
        batch: &mut FetchedBatch,
    ) -> Result<()> {
        let mailbox = session.examine(name).await.map_err(err)?;
        let uid_validity = mailbox.uid_validity.unwrap_or(0);
        let after = match batch.cursor.mailboxes.get(name) {
            Some(MailboxCursor::Imap {
                uid_validity: held,
                last_uid,
            }) if *held == uid_validity => *last_uid,
            _ => 0,
        };
        // `n:*` always matches the newest mail, even if its UID is below `n`.
        let found = match uids_after(after) {
            Some(search) => session.uid_search(search).await.map_err(err)?,
            None => std::collections::HashSet::new(),
        };
        let mut uids: Vec<u32> = found.into_iter().filter(|u| *u > after).collect();
        uids.sort_unstable();
        let more = uids.len() > limits.mails;
        uids.truncate(limits.mails);
        let last_uid = uids.last().copied().unwrap_or(after);
        if !uids.is_empty() {
            let set = uids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let sizes = Self::sizes(session, &set).await?;
            let bytes = u64::try_from(limits.bytes).unwrap_or(u64::MAX);
            let over = |uid: &u32| sizes.get(uid).is_some_and(|size| u64::from(*size) > bytes);
            let (large, fitting): (Vec<u32>, Vec<u32>) = uids.iter().partition(|uid| over(uid));
            let at = (name, uid_validity, role);
            Self::read_bodies(session, at, &fitting, "BODY.PEEK[]", false, batch).await?;
            // One byte past the limit, so that the core sees a mail over it.
            let start = format!("BODY.PEEK[]<0.{}>", limits.bytes.saturating_add(1));
            Self::read_bodies(session, at, &large, &start, true, batch).await?;
        }
        batch.more |= more;
        batch.cursor.mailboxes.insert(
            name.to_string(),
            MailboxCursor::Imap {
                uid_validity,
                last_uid,
            },
        );
        Ok(())
    }

    async fn read_bodies(
        session: &mut Session<Stream>,
        (name, uid_validity, role): (&str, u32, MailboxRole),
        uids: &[u32],
        section: &str,
        truncated: bool,
        batch: &mut FetchedBatch,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetched: Vec<_> = session
            .uid_fetch(&set, format!("(UID {section})"))
            .await
            .map_err(err)?
            .try_collect()
            .await
            .map_err(err)?;
        let mut mails: Vec<(u32, Vec<u8>)> = fetched
            .iter()
            .filter_map(|f| Some((f.uid?, f.body()?.to_vec())))
            .collect();
        mails.sort_unstable_by_key(|(uid, _)| *uid);
        if mails.len() != uids.len() {
            return Err(err(format!(
                "{name}: asked for {} mails, got {}; the cursor stays",
                uids.len(),
                mails.len()
            )));
        }
        for (uid, bytes) in mails {
            batch.mails.push(FetchedMail {
                bytes,
                role,
                stable_id: format!("{name}/{uid_validity}/{uid}"),
                truncated,
            });
        }
        Ok(())
    }

    /// `work`, or an error when the configured time is up.
    async fn in_time<T>(&self, work: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(self.config.timeout, work)
            .await
            .map_err(|_| err(format!("no answer within {:?}", self.config.timeout)))?
    }
}

#[async_trait]
impl MailFetch for ImapFetch {
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError> {
        self.in_time(async {
            let mut session = self.session().await?;
            let sent = Self::sent_mailbox(&mut session).await?;
            let mut batch = FetchedBatch {
                cursor: cursor.clone(),
                ..FetchedBatch::default()
            };
            Self::read_mailbox(&mut session, INBOX, MailboxRole::Inbox, limits, &mut batch).await?;
            if let Some(sent) = sent {
                let read =
                    Self::read_mailbox(&mut session, &sent, MailboxRole::Sent, limits, &mut batch);
                sent_is_optional(read.await);
            }
            let _ = session.logout().await;
            Ok(batch)
        })
        .await
    }

    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError> {
        self.in_time(async {
            let mut session = self.session().await?;
            let sent = Self::sent_mailbox(&mut session)
                .await?
                .ok_or_else(|| err("the server names no mailbox for sent mail"))?;
            session
                .append(&sent, Some("(\\Seen)"), None, raw)
                .await
                .map_err(err)?;
            let _ = session.logout().await;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_comes_after_the_highest_uid() {
        assert_eq!(uids_after(5).as_deref(), Some("UID 6:*"));
        assert_eq!(uids_after(u32::MAX), None);
    }
}
