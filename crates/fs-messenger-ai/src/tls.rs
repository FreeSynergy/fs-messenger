//! [`TlsTrust`]: whose certificates count.
//!
//! Pattern: Strategy: which roots count is one choice, and there is no "accept invalid". The system
//! roots are the only option of a normal build; trusting exactly one given certificate exists only under
//! the `test-trust` feature, for the scripted server. The same choice as in `fs-messenger-mail`.

use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};

use crate::error::AiError;

/// Whose certificates count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TlsTrust {
    /// The roots of the operating system.
    #[default]
    SystemRoots,
    /// TEST ONLY: trust this one certificate (PEM) and nothing else. The host name is still checked.
    #[cfg(feature = "test-trust")]
    OnlyThisCertificateForTests(String),
}

fn tls(e: impl std::fmt::Display) -> AiError {
    AiError::Tls(e.to_string())
}

impl TlsTrust {
    fn root_store(&self) -> Result<RootCertStore, AiError> {
        let mut roots = RootCertStore::empty();
        match self {
            Self::SystemRoots => {
                let found = rustls_native_certs::load_native_certs();
                let (added, _ignored) = roots.add_parsable_certificates(found.certs);
                if added == 0 {
                    return Err(tls("no usable system root certificates"));
                }
            }
            #[cfg(feature = "test-trust")]
            Self::OnlyThisCertificateForTests(pem) => {
                use rustls::pki_types::{CertificateDer, pem::PemObject};
                for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
                    roots.add(cert.map_err(tls)?).map_err(tls)?;
                }
                if roots.is_empty() {
                    return Err(tls("no certificate in the given PEM"));
                }
            }
        }
        Ok(roots)
    }

    /// A rustls configuration with only these roots; without TLS on the way, with none at all. Uses
    /// ring explicitly, so no process-wide default provider is needed.
    pub(crate) fn client_config(&self, uses_tls: bool) -> Result<ClientConfig, AiError> {
        let roots = if uses_tls {
            self.root_store()?
        } else {
            RootCertStore::empty()
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        Ok(ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls)?
            .with_root_certificates(roots)
            .with_no_client_auth())
    }
}
