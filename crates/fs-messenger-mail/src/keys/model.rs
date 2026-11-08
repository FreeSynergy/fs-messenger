//! What is held about a peer: [`PeerKey`], [`CandidateKey`], [`KeySource`].
//!
//! Pattern: Value objects. Public data only; secrets stay behind the [`super::KeyStore`].

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::error::{MailError, Result};
use crate::openpgp::PublicCert;

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

/// The v4 key held beside a v6 key: what an older program of the same person reads. Never the key in
/// use for signing proofs; it is added and replaced only with a proof (`autocrypt::rule`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyKey {
    /// Fingerprint of the v4 key.
    pub fingerprint: String,
    /// The certificate, binary, base64.
    pub cert: String,
}

impl LegacyKey {
    /// Hold `cert` as the v4 key.
    pub fn new(cert: &PublicCert) -> Result<Self, MailError> {
        Ok(Self {
            fingerprint: cert.fingerprint(),
            cert: STANDARD.encode(cert.to_binary()?),
        })
    }

    /// The certificate.
    pub fn certificate(&self) -> Result<PublicCert, MailError> {
        decode_cert(&self.cert)
    }
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
    /// The peer's v4 key beside the key in use, if that one is v6.
    #[serde(default)]
    pub legacy: Option<LegacyKey>,
}

impl PeerKey {
    /// A record for a key seen now.
    pub fn new(
        address: &str,
        cert: &PublicCert,
        source: KeySource,
        now: u64,
    ) -> Result<Self, MailError> {
        Ok(Self {
            address: crate::address::normalise_address(address),
            fingerprint: cert.fingerprint(),
            cert: STANDARD.encode(cert.to_binary()?),
            source,
            first_seen: now,
            last_seen: now,
            candidate: None,
            legacy: None,
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
