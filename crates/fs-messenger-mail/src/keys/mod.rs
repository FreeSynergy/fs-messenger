//! Key storage: the [`KeyStore`] trait and what it holds.
//!
//! Pattern: Repository (interface) with value objects. A person's own secret and what we know of each
//! peer. Secrets stay behind this trait; [`PeerKey`] holds public data only.

mod store;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::error::{MailError, Result};
use crate::openpgp::{PublicCert, SecretCert};

pub use store::FsKeyStore;

/// How a key came to be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeySource {
    /// First seen in an Autocrypt header. Nobody checked it: trust on first use.
    AutocryptFirstSeen,
    /// Replaced the earlier key, proven by a signature of that key over the new one.
    AutocryptRotated,
    /// A person decided: imported by hand, or a candidate accepted.
    Person,
}

/// A key that was offered and not taken.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateKey {
    /// Fingerprint of the offered key.
    pub fingerprint: String,
    /// The certificate, binary, base64.
    pub cert: String,
    /// When it was last offered, seconds since the epoch.
    pub last_offered: u64,
}

/// What we hold about one peer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerKey {
    /// The address, normalised.
    pub address: String,
    /// Fingerprint of the key in use.
    pub fingerprint: String,
    /// The certificate in use, binary, base64.
    pub cert: String,
    /// How it came to be trusted.
    pub source: KeySource,
    /// When it was first seen, seconds since the epoch.
    pub first_seen: u64,
    /// When it was last seen, seconds since the epoch.
    pub last_seen: u64,
    /// A different key somebody offered without proof. Never encrypted to; shown as a conflict.
    pub candidate: Option<CandidateKey>,
}

impl PeerKey {
    /// A record for a key seen now.
    pub fn new(address: &str, cert: &PublicCert, source: KeySource, now: u64) -> Result<Self, MailError> {
        Ok(Self {
            address: crate::address::normalise_address(address),
            fingerprint: cert.fingerprint(),
            cert: STANDARD.encode(cert.to_binary()?),
            source,
            first_seen: now,
            last_seen: now,
            candidate: None,
        })
    }

    /// The certificate in use.
    pub fn certificate(&self) -> Result<PublicCert, MailError> {
        decode_cert(&self.cert)
    }
}

pub(crate) fn decode_cert(text: &str) -> Result<PublicCert> {
    let bytes = STANDARD
        .decode(text)
        .map_err(|e| MailError::Keys(e.to_string()))?;
    PublicCert::from_binary(&bytes)
}

/// Where keys live.
#[async_trait]
pub trait KeyStore: Send + Sync {
    /// The person's own secret key for `address`.
    async fn own_key(&self, address: &str) -> Result<Option<SecretCert>, MailError>;
    /// Keep the person's own secret key for `address`, replacing an earlier one.
    async fn put_own_key(&self, address: &str, key: &SecretCert) -> Result<(), MailError>;
    /// What is known about a peer.
    async fn peer(&self, address: &str) -> Result<Option<PeerKey>, MailError>;
    /// Keep what is known about a peer, replacing the earlier record.
    async fn put_peer(&self, peer: &PeerKey) -> Result<(), MailError>;
}
