//! [`PublicCert`] and [`SecretCert`]: certificates as this crate passes them around.
//!
//! Pattern: Newtype (Adapter over rPGP's types). A [`SecretCert`] cannot be printed: its `Debug` and
//! `Display` show the fingerprint only.

use std::fmt;
use std::io::Cursor;

use pgp::composed::{Deserializable, SignedPublicKey, SignedSecretKey};
use pgp::ser::Serialize;
use pgp::types::KeyDetails;

use crate::error::{MailError, Result};

/// A certificate larger than this is not looked at, and no merge makes one.
pub(crate) const MAX_CERT_BYTES: usize = 64 * 1024;

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

    /// Whether this is a v6 key (RFC 9580). Anything else is an older one.
    pub fn is_v6(&self) -> bool {
        self.0.version() == pgp::types::KeyVersion::V6
    }

    /// Whether this is a v4 key (RFC 4880).
    pub fn is_v4(&self) -> bool {
        self.0.version() == pgp::types::KeyVersion::V4
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

impl fmt::Display for SecretCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "secret key {}, <redacted>", self.fingerprint())
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}
