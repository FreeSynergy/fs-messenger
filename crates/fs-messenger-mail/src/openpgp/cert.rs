//! [`PublicCert`] and [`SecretCert`]: certificates as this crate passes them around.
//!
//! Pattern: Newtype (Adapter over rPGP's types). A [`SecretCert`] cannot be printed: its `Debug` and
//! `Display` show the fingerprint only.

use std::fmt;
use std::io::Cursor;

use pgp::composed::{Deserializable, SignedPublicKey, SignedPublicSubKey, SignedSecretKey};
use pgp::packet::{Signature, Subpacket, SubpacketData};
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
    /// This certificate and everything `other` adds to the same key: signatures and subkeys. `None` if
    /// it adds nothing, if the result is over the size limit, or if it does not verify. Nothing is ever
    /// removed, so a form with a subkey stripped cannot make a peer unusable, and a form without a
    /// revocation cannot undo one.
    pub(crate) fn merged_with(&self, other: &Self) -> Option<Self> {
        self.merged_within(other, MAX_CERT_BYTES)
    }

    pub(super) fn merged_within(&self, other: &Self, cap: usize) -> Option<Self> {
        let mut merged = self.clone();
        let fits = |c: &Self| c.to_binary().is_ok_and(|b| b.len() <= cap);
        (merged.merge(other) && fits(&merged) && merged.0.verify_bindings().is_ok())
            .then_some(merged)
    }

    /// The same certificate without what its signatures do not cover, for keeping. As offered if that
    /// no longer verifies.
    pub(crate) fn compact(&self) -> Self {
        let mut compact = self.clone();
        let details = &mut compact.0.details;
        compact_all(&mut details.revocation_signatures);
        compact_all(&mut details.direct_signatures);
        details
            .users
            .iter_mut()
            .for_each(|u| compact_all(&mut u.signatures));
        compact
            .0
            .public_subkeys
            .iter_mut()
            .for_each(|s| compact_all(&mut s.signatures));
        if compact.0.verify_bindings().is_ok() {
            compact
        } else {
            self.clone()
        }
    }

    fn merge(&mut self, other: &Self) -> bool {
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
            if let Some(mine) = held.details.users.iter_mut().find(|u| u.id == user.id) {
                changed |= add_missing(&mut mine.signatures, &user.signatures);
            } else {
                let mut new = user.clone();
                compact_all(&mut new.signatures);
                held.details.users.push(new);
                changed = true;
            }
        }
        for sub in &offered.public_subkeys {
            let same = |s: &&mut SignedPublicSubKey| s.key.fingerprint() == sub.key.fingerprint();
            if let Some(mine) = held.public_subkeys.iter_mut().find(|s| same(s)) {
                changed |= add_missing(&mut mine.signatures, &sub.signatures);
            } else {
                let mut new = sub.clone();
                compact_all(&mut new.signatures);
                held.public_subkeys.push(new);
                changed = true;
            }
        }
        changed
    }
}

/// What a signature covers: its hashed area and its bytes. The unhashed area is covered by nothing, so
/// anyone can write there without a key, and two signatures differing only there are one signature.
fn same_signature(a: &Signature, b: &Signature) -> bool {
    let covered = |s: &Signature| {
        s.config().map(|c| {
            let mut c = c.clone();
            c.unhashed_subpackets.clear();
            c
        })
    };
    let (ca, cb) = (covered(a), covered(b));
    ca.is_some() && ca == cb && a.signature() == b.signature()
}

/// `signature` without its unhashed area. rPGP also takes a signing subkey's back-signature from there,
/// where it is safe, since it proves itself; so that one stays, stripped in turn.
fn compact_signature(signature: &Signature) -> Option<Signature> {
    let mut config = signature.config()?.clone();
    let hashed_back = config
        .hashed_subpackets()
        .any(|p| matches!(p.data, SubpacketData::EmbeddedSignature(_)));
    let back = signature
        .embedded_signature()
        .filter(|_| !hashed_back)
        .and_then(compact_signature)
        .and_then(|b| Subpacket::regular(SubpacketData::EmbeddedSignature(Box::new(b))).ok());
    config.unhashed_subpackets = back.into_iter().collect();
    let (left, bytes) = (
        signature.signed_hash_value()?,
        signature.signature()?.clone(),
    );
    Signature::from_config(config, left, bytes).ok()
}

fn compact_all(signatures: &mut Vec<Signature>) {
    *signatures = signatures.iter().filter_map(compact_signature).collect();
}

fn add_missing(held: &mut Vec<Signature>, offered: &[Signature]) -> bool {
    let before = held.len();
    for signature in offered {
        if !held.iter().any(|h| same_signature(h, signature))
            && let Some(compact) = compact_signature(signature)
        {
            held.push(compact);
        }
    }
    held.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openpgp::generate_key;

    /// What anyone can write without a key: a copy of the direct signature, padded in the unhashed area.
    fn padded(cert: &PublicCert, tag: &str) -> PublicCert {
        let mut form = cert.clone();
        let signature = &form.0.details.direct_signatures[0];
        let mut config = signature.config().unwrap().clone();
        let note = pgp::packet::Notation {
            readable: true,
            name: "pad@example.org".as_bytes().to_vec().into(),
            value: tag.as_bytes().to_vec().into(),
        };
        config
            .unhashed_subpackets
            .push(Subpacket::regular(SubpacketData::Notation(note)).unwrap());
        let copy = Signature::from_config(
            config,
            signature.signed_hash_value().unwrap(),
            signature.signature().unwrap().clone(),
        )
        .unwrap();
        form.0.details.direct_signatures.push(copy);
        form
    }

    #[test]
    fn a_signature_padded_in_the_unhashed_area_is_the_same_signature() {
        let cert = generate_key("a@x").unwrap().public();
        let form = padded(&cert, "anything");
        assert!(
            form.0.verify_bindings().is_ok(),
            "the padding verifies, it is not covered"
        );
        assert!(cert.merged_with(&form).is_none(), "and it adds nothing");
    }

    #[test]
    fn compacting_drops_what_is_not_covered_and_still_verifies() {
        let key = generate_key("a@x").unwrap();
        let form = padded(&key.public(), &"x".repeat(5000));
        let compact = form.compact();
        assert!(compact.0.verify_bindings().is_ok());
        assert!(compact.to_binary().unwrap().len() < 2000);
        assert_eq!(compact.fingerprint(), key.fingerprint());
    }
}
