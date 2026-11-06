//! Where a server is and whom to trust: [`Endpoint`], [`TlsTrust`].
//!
//! Pattern: Strategy: which roots count is one choice, and there is no "accept invalid". The system
//! roots are the only option of a normal build; trusting exactly one given certificate exists only
//! under the `test-trust` feature, for the self-signed test server.

use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};

use crate::error::{MailError, Result};

/// Whose certificates count.
#[derive(Clone, Debug, Default)]
pub enum TlsTrust {
    /// The roots of the operating system.
    #[default]
    SystemRoots,
    /// TEST ONLY: trust this one certificate (PEM) and nothing else. The host name is still checked.
    #[cfg(feature = "test-trust")]
    OnlyThisCertificateForTests(String),
}

/// A server with implicit TLS. STARTTLS is `Unsupported`: the test server offers no such port, so it
/// could not be verified.
#[derive(Clone, Debug)]
pub struct Endpoint {
    /// Host name; it is also the name the certificate must carry.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Whose certificates count.
    pub trust: TlsTrust,
}

impl TlsTrust {
    fn root_store(&self) -> Result<RootCertStore> {
        let mut roots = RootCertStore::empty();
        match self {
            Self::SystemRoots => {
                let found = rustls_native_certs::load_native_certs();
                let (added, _ignored) = roots.add_parsable_certificates(found.certs);
                if added == 0 {
                    return Err(MailError::Tls("no usable system root certificates".into()));
                }
            }
            #[cfg(feature = "test-trust")]
            Self::OnlyThisCertificateForTests(pem) => {
                use rustls::pki_types::{CertificateDer, pem::PemObject};
                for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
                    let cert = cert.map_err(|e| MailError::Tls(e.to_string()))?;
                    roots.add(cert).map_err(|e| MailError::Tls(e.to_string()))?;
                }
                if roots.is_empty() {
                    return Err(MailError::Tls("no certificate in the given PEM".into()));
                }
            }
        }
        Ok(roots)
    }

    /// A rustls client configuration with only these roots. Uses ring explicitly, so no process-wide
    /// default provider is needed.
    pub(crate) fn client_config(&self) -> Result<ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        Ok(ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| MailError::Tls(e.to_string()))?
            .with_root_certificates(self.root_store()?)
            .with_no_client_auth())
    }
}
