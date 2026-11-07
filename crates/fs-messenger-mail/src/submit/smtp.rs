//! [`SmtpSubmit`]: the SMTP strategy, with `lettre` over rustls.
//!
//! Pattern: Strategy + Adapter. Implicit TLS only; certificates are checked through the roots of
//! [`TlsTrust`]. `lettre`'s `builder_dangerous` is its name for "no default TLS": we always set it.

use async_trait::async_trait;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
#[cfg(feature = "test-trust")]
use lettre::transport::smtp::client::Certificate;
use lettre::transport::smtp::client::{CertificateStore, Tls, TlsParameters};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use super::{MailEnvelope, MailSubmit, SentCopy};
use crate::error::{MailError, Result};
use crate::secret::Password;
use crate::tls::{Endpoint, TlsTrust};

/// How to reach an SMTP submission server. Implicit TLS only.
#[derive(Clone, Debug)]
pub struct SmtpConfig {
    /// The server.
    pub endpoint: Endpoint,
    /// Login name.
    pub user: String,
    /// Login secret.
    pub password: Password,
}

/// Submits over SMTP. SMTP keeps no sent copy.
pub struct SmtpSubmit {
    config: SmtpConfig,
}

fn err(e: impl std::fmt::Display) -> MailError {
    MailError::Smtp(e.to_string())
}

impl SmtpSubmit {
    /// A strategy for `config`. Connects on use.
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    fn tls(&self) -> Result<Tls> {
        let endpoint = &self.config.endpoint;
        let builder = TlsParameters::builder(endpoint.host.clone());
        let builder = match &endpoint.trust {
            TlsTrust::SystemRoots => builder.certificate_store(CertificateStore::Default),
            #[cfg(feature = "test-trust")]
            TlsTrust::OnlyThisCertificateForTests(pem) => builder
                .certificate_store(CertificateStore::None)
                .add_root_certificate(Certificate::from_pem(pem.as_bytes()).map_err(err)?),
        };
        Ok(Tls::Wrapper(builder.build_rustls().map_err(err)?))
    }

    fn transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
        let endpoint = &self.config.endpoint;
        let credentials = Credentials::new(
            self.config.user.clone(),
            self.config.password.expose().to_string(),
        );
        Ok(
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(endpoint.host.as_str())
                .port(endpoint.port)
                .tls(self.tls()?)
                .credentials(credentials)
                .build(),
        )
    }
}

fn parsed(address: &str) -> Result<Address> {
    address.parse().map_err(err)
}

#[async_trait]
impl MailSubmit for SmtpSubmit {
    async fn submit(&self, envelope: &MailEnvelope, raw: &[u8]) -> Result<SentCopy, MailError> {
        let to = envelope
            .recipients
            .iter()
            .map(|r| parsed(r))
            .collect::<Result<Vec<_>>>()?;
        let wire = Envelope::new(Some(parsed(&envelope.from)?), to).map_err(err)?;
        self.transport()?.send_raw(&wire, raw).await.map_err(err)?;
        Ok(SentCopy::NotStored)
    }
}
