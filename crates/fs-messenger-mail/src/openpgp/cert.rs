//! [`PublicCert`] and [`SecretCert`]: certificates as this crate passes them around.
//!
//! Pattern: Newtype (Adapter over rPGP's types). A [`SecretCert`] cannot be printed: its `Debug` shows
//! the fingerprint only, and it has no `Display`.

use std::fmt;
use std::io::Cursor;

use pgp::composed::{Deserializable, SignedPublicKey, SignedSecretKey};
use pgp::ser::Serialize;
use pgp::types::KeyDetails;

use crate::error::{MailError, Result};

/// A public certificate: a primary key with its subkeys and self-signatures.
#[derive(Clone, PartialEq, Eq)]
pub struct PublicCert(pub(crate) SignedPublicKey);

impl PublicCert {
    /// Read the binary form, as in an Autocrypt `keydata`.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, MailError> {
        Ok(Self(SignedPublicKey::from_bytes(Cursor::new(bytes))?))
    }

    /// The binary form.
    pub fn to_binary(&self) -> Result<Vec<u8>, MailError> {
        Ok(self.0.to_bytes()?)
    }

    /// Fingerprint in lower-case hex.
    pub fn fingerprint(&self) -> String {
        hex(self.0.fingerprint().as_bytes())
    }
}

impl fmt::Debug for PublicCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicCert({})", self.fingerprint())
    }
}

/// A secret key. Lives in memory and in an encrypted store only.
#[derive(Clone)]
pub struct SecretCert(pub(crate) SignedSecretKey);

impl SecretCert {
    /// Read the binary form, as stored.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, MailError> {
        Ok(Self(SignedSecretKey::from_bytes(Cursor::new(bytes))?))
    }

    /// The binary form, for an encrypted store only.
    pub(crate) fn to_binary(&self) -> Result<Vec<u8>> {
        Ok(self.0.to_bytes()?)
    }

    /// The public half.
    pub fn public(&self) -> PublicCert {
        PublicCert(self.0.to_public_key())
    }

    /// Fingerprint in lower-case hex.
    pub fn fingerprint(&self) -> String {
        hex(self.0.fingerprint().as_bytes())
    }
}

impl fmt::Debug for SecretCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretCert({}, <redacted>)", self.fingerprint())
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

impl PublicCert {
    /// Creation time of the newest self-signature, for telling two forms of one key apart.
    pub(crate) fn newest_self_signature(&self) -> u64 {
        let details = &self.0.details;
        details
            .direct_signatures
            .iter()
            .chain(details.users.iter().flat_map(|u| u.signatures.iter()))
            .filter_map(pgp::packet::Signature::created)
            .map(|t| u64::from(t.as_secs()))
            .max()
            .unwrap_or(0)
    }
}
