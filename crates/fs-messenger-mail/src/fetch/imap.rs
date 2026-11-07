//! [`ImapFetch`]: the IMAP strategy, with `async-imap` over rustls.
//!
//! Pattern: Strategy + Adapter. One connection per call, no pool; push through IDLE is `Unsupported`.
//! Every byte runs under an idle timeout: a server that goes silent must not stall the account for ever,
//! and a large mail on a slow line may still take its time.

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

use super::idle::IdleStream;
use super::imap_mailbox::read_mailbox;
use super::{
    Budget, FetchLimits, FetchedBatch, MailCursor, MailFetch, MailboxRole, sent_is_optional,
};
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
    /// How long the connection may stand still, the handshake included. [`super::DEFAULT_IDLE_TIMEOUT`].
    pub idle_timeout: Duration,
}

/// Reads mail over IMAP.
pub struct ImapFetch {
    config: ImapConfig,
}

pub(super) fn err(e: impl std::fmt::Display) -> MailError {
    MailError::Imap(e.to_string())
}

pub(super) type Stream = TlsStream<IdleStream<TcpStream>>;

impl ImapFetch {
    /// A strategy for `config`. Connects on use.
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    async fn session(&self) -> Result<Session<Stream>> {
        let endpoint = &self.config.endpoint;
        let limit = self.config.idle_timeout;
        let tls = TlsConnector::from(Arc::new(endpoint.trust.client_config()?));
        let name = ServerName::try_from(endpoint.host.clone()).map_err(err)?;
        let connecting = TcpStream::connect((endpoint.host.as_str(), endpoint.port));
        let tcp = tokio::time::timeout(limit, connecting)
            .await
            .map_err(|_| err(format!("no connection within {limit:?}")))?
            .map_err(err)?;
        let stream = tls
            .connect(name, IdleStream::new(tcp, limit))
            .await
            .map_err(err)?;
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
}

#[async_trait]
impl MailFetch for ImapFetch {
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError> {
        let mut session = self.session().await?;
        let sent = Self::sent_mailbox(&mut session).await?;
        let mut batch = FetchedBatch {
            cursor: cursor.clone(),
            ..FetchedBatch::default()
        };
        let mut budget = Budget::new(limits.batch_bytes);
        read_mailbox(
            &mut session,
            (INBOX, MailboxRole::Inbox),
            limits,
            &mut budget,
            &mut batch,
        )
        .await?;
        if let Some(sent) = sent {
            let read = read_mailbox(
                &mut session,
                (&sent, MailboxRole::Sent),
                limits,
                &mut budget,
                &mut batch,
            );
            sent_is_optional(read.await);
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
