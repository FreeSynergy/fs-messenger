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

impl PublicCert {
    /// This certificate and everything `other` adds to the same key: signatures and subkeys, each
    /// verified by `verify_bindings` before anyone relies on it. Nothing is ever removed, so a form with
    /// a subkey stripped cannot make a peer unusable, and a form without a revocation cannot undo one.
    /// Answers whether anything was added.
    pub(crate) fn merge(&mut self, other: &Self) -> bool {
        let (held, offered) = (&mut self.0, &other.0);
        let mut changed = add_missing(
            &mut held.details.revocation_signatures,
            &offered.details.revocation_signatures,
        );
        changed |= add_missing(
            &mut held.details.direct_signatures,
            &offered.details.direct_signatures,
        );
        for user in &offered.details.users {
            match held.details.users.iter_mut().find(|u| u.id == user.id) {
                Some(mine) => changed |= add_missing(&mut mine.signatures, &user.signatures),
                None => {
                    held.details.users.push(user.clone());
                    changed = true;
                }
            }
        }
        for sub in &offered.public_subkeys {
            let same = |s: &&mut pgp::composed::SignedPublicSubKey| {
                s.key.fingerprint() == sub.key.fingerprint()
            };
            match held.public_subkeys.iter_mut().find(|s| same(s)) {
                Some(mine) => changed |= add_missing(&mut mine.signatures, &sub.signatures),
                None => {
                    held.public_subkeys.push(sub.clone());
                    changed = true;
                }
            }
        }
        changed
    }
}

fn add_missing<T: Clone + PartialEq>(held: &mut Vec<T>, offered: &[T]) -> bool {
    let before = held.len();
    for item in offered {
        if !held.contains(item) {
            held.push(item.clone());
        }
    }
    held.len() != before
}
