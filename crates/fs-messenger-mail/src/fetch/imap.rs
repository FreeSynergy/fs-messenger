//! [`ImapFetch`]: the IMAP strategy, with `async-imap` over rustls.
//!
//! Pattern: Strategy + Adapter. One connection per call, no pool; push through IDLE is `Unsupported`.
//! Mailboxes are opened read-only (`EXAMINE`, `BODY.PEEK`), so fetching marks nothing as read.

use std::sync::Arc;

use async_imap::Session;
use async_imap::imap_proto::NameAttribute;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::{FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole};
use crate::error::{MailError, Result};
use crate::secret::Password;
use crate::tls::Endpoint;

const INBOX: &str = "INBOX";

/// How to reach an IMAP server. Implicit TLS only.
#[derive(Clone, Debug)]
pub struct ImapConfig {
    /// The server.
    pub endpoint: Endpoint,
    /// Login name.
    pub user: String,
    /// Login secret.
    pub password: Password,
}

/// Reads mail over IMAP.
pub struct ImapFetch {
    config: ImapConfig,
}

fn err(e: impl std::fmt::Display) -> MailError {
    MailError::Imap(e.to_string())
}

type Stream = TlsStream<TcpStream>;

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

    async fn read_mailbox(
        session: &mut Session<Stream>,
        name: &str,
        role: MailboxRole,
        limit: usize,
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
        let found = session
            .uid_search(format!("UID {}:*", after + 1))
            .await
            .map_err(err)?;
        let mut uids: Vec<u32> = found.into_iter().filter(|u| *u > after).collect();
        uids.sort_unstable();
        batch.more |= uids.len() > limit;
        uids.truncate(limit);
        let mut last_uid = after;
        if !uids.is_empty() {
            let set = uids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let fetched: Vec<_> = session
                .uid_fetch(&set, "(UID BODY.PEEK[])")
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
                last_uid = uid;
                batch.mails.push(FetchedMail {
                    bytes,
                    role,
                    stable_id: format!("{name}/{uid_validity}/{uid}"),
                });
            }
        }
        batch.cursor.mailboxes.insert(
            name.to_string(),
            MailboxCursor::Imap {
                uid_validity,
                last_uid,
            },
        );
        Ok(())
    }
}

#[async_trait]
impl MailFetch for ImapFetch {
    async fn fetch(&self, cursor: &MailCursor, limit: usize) -> Result<FetchedBatch, MailError> {
        let mut session = self.session().await?;
        let sent = Self::sent_mailbox(&mut session).await?;
        let mut batch = FetchedBatch {
            cursor: cursor.clone(),
            ..FetchedBatch::default()
        };
        Self::read_mailbox(&mut session, INBOX, MailboxRole::Inbox, limit, &mut batch).await?;
        if let Some(sent) = sent {
            Self::read_mailbox(&mut session, &sent, MailboxRole::Sent, limit, &mut batch).await?;
        }
        let _ = session.logout().await;
        Ok(batch)
    }

    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError> {
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
    }
}
