//! Whether a certificate may be encrypted to now: [`usable_encryption_key`].
//!
//! Pattern: Specification. rPGP gives the packets, not the semantics, so the rule is written here:
//! every self-signature must verify, the key must not be revoked, expired or of a broken algorithm, and
//! a live encryption subkey must exist. A subkey with a revocation is skipped; `verify_bindings` has
//! verified that signature with the rest. Provisional: a key that encrypts only with its primary key is
//! refused, and a revoked subkey stays revoked even if a later binding says otherwise. A v4 key that does not advertise `SEIPDv2` is refused:
//! `SEIPDv1` is `Unsupported` outgoing.

use pgp::composed::SignedPublicSubKey;
use pgp::packet::{Signature, SignatureType};
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails, KeyVersion, PublicParams, VerifyingKey};

use super::PublicCert;

/// Why a key is not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyRefusal {
    /// A self-signature does not verify.
    #[error("its signatures do not verify")]
    Invalid,
    /// The owner revoked it.
    #[error("it is revoked")]
    Revoked,
    /// It or its encryption subkey is expired.
    #[error("it is expired")]
    Expired,
    /// It has no live subkey for encryption.
    #[error("it cannot encrypt")]
    CannotEncrypt,
    /// DSA, Elgamal, or RSA under 2048 bits.
    #[error("its algorithm or key size is too weak")]
    Weak,
    /// A v4 key that does not advertise `SEIPDv2`.
    #[error("it does not support SEIPDv2")]
    NoModernEncryption,
}

/// Seconds since the epoch.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn newest<'a>(sigs: impl Iterator<Item = &'a Signature>) -> Option<&'a Signature> {
    sigs.max_by_key(|s| s.created().map_or(0, |t| u64::from(t.as_secs())))
}

/// Whether a key created at `created` with this self-signature has run out at `now`.
fn expired(created: u64, sig: &Signature, now: u64) -> bool {
    let lifetime = sig
        .key_expiration_time()
        .map_or(0, |d| u64::from(d.as_secs()));
    lifetime != 0 && created + lifetime <= now
}

const MIN_RSA_BITS: usize = 2048;

/// Whether these parameters are of an algorithm and size we rely on. rPGP parses them all.
fn strong(params: &PublicParams) -> bool {
    match params {
        PublicParams::DSA(_) | PublicParams::Elgamal(_) => false,
        // An RSA key starts with the MPI of `n`, whose first two bytes are its length in bits.
        PublicParams::RSA(_) => params.to_bytes().is_ok_and(|b| {
            b.len() >= 2 && usize::from(u16::from_be_bytes([b[0], b[1]])) >= MIN_RSA_BITS
        }),
        _ => true,
    }
}

/// Whether a signature by this key says anything: its signatures verify, it is not revoked and not of
/// a broken algorithm. An expired key still counts here, for what it signed while it lived.
pub(crate) fn may_vouch(cert: &PublicCert) -> bool {
    cert.0.verify_bindings().is_ok()
        && cert.0.details.revocation_signatures.is_empty()
        && strong(cert.0.primary_key.public_params())
}

/// A key of a certificate that may sign, and until when.
pub(crate) struct SigningKey<'a> {
    key: Signer<'a>,
    /// When its life ends, if it does.
    until: Option<u64>,
}

enum Signer<'a> {
    Primary(&'a pgp::packet::PublicKey),
    Subkey(&'a SignedPublicSubKey),
}

impl SigningKey<'_> {
    pub(crate) fn key(&self) -> &dyn VerifyingKey {
        match self.key {
            Signer::Primary(k) => k,
            Signer::Subkey(k) => k,
        }
    }

    /// Whether `signature` over `content` verifies with this key, and was made while it lived.
    pub(crate) fn verifies(&self, signature: &Signature, content: &[u8]) -> bool {
        let data = std::io::Cursor::new(content);
        self.covers(signature)
            && match self.key {
                Signer::Primary(k) => signature.verify(k, data).is_ok(),
                Signer::Subkey(k) => signature.verify(k, data).is_ok(),
            }
    }

    /// Whether the signature was made while the key lived. Its date is the signer's word: whoever
    /// stole an expired key can write an older one (`DESIGN.md`).
    pub(crate) fn covers(&self, signature: &Signature) -> bool {
        let made = signature.created().map(|t| u64::from(t.as_secs()));
        match (self.until, made) {
            (None, _) => true,
            (Some(until), Some(made)) => made < until,
            (Some(_), None) => false,
        }
    }
}

fn life_end(created: u64, sig: &Signature) -> Option<u64> {
    let lifetime = sig
        .key_expiration_time()
        .map_or(0, |d| u64::from(d.as_secs()));
    (lifetime != 0).then_some(created + lifetime)
}

/// The keys of `cert` a signature may be made with: those whose own self-signature carries the sign
/// flag, without a revocation, in a certificate whose signatures verify. An encryption or authentication
/// subkey signs nothing, and a revoked one proves nothing. Whether the whole key is revoked is
/// `may_vouch`.
pub(crate) fn signing_keys(cert: &PublicCert) -> Vec<SigningKey<'_>> {
    let mut keys: Vec<SigningKey<'_>> = vec![];
    if cert.0.verify_bindings().is_err() {
        return keys;
    }
    if let Some(own) = self_signature(cert).filter(|s| s.key_flags().sign()) {
        let created = u64::from(cert.0.primary_key.created_at().as_secs());
        keys.push(SigningKey {
            key: Signer::Primary(&cert.0.primary_key),
            until: life_end(created, own),
        });
    }
    for sub in &cert.0.public_subkeys {
        let revoked = sub
            .signatures
            .iter()
            .any(|s| s.typ() == Some(SignatureType::SubkeyRevocation));
        let bindings = sub
            .signatures
            .iter()
            .filter(|s| s.typ() == Some(SignatureType::SubkeyBinding));
        if let Some(binding) = newest(bindings).filter(|s| !revoked && s.key_flags().sign()) {
            let created = u64::from(sub.key.created_at().as_secs());
            keys.push(SigningKey {
                key: Signer::Subkey(sub),
                until: life_end(created, binding),
            });
        }
    }
    keys
}

fn self_signature(cert: &PublicCert) -> Option<&Signature> {
    let details = &cert.0.details;
    let direct = newest(details.direct_signatures.iter());
    let user = newest(details.users.iter().flat_map(|u| u.signatures.iter()));
    direct.or(user)
}

/// The key to encrypt to, or why there is none, at `now`.
pub fn usable_encryption_key(
    cert: &PublicCert,
    now: u64,
) -> Result<&SignedPublicSubKey, KeyRefusal> {
    if cert.0.verify_bindings().is_err() {
        return Err(KeyRefusal::Invalid);
    }
    if !cert.0.details.revocation_signatures.is_empty() {
        return Err(KeyRefusal::Revoked);
    }
    if !strong(cert.0.primary_key.public_params()) {
        return Err(KeyRefusal::Weak);
    }
    let created = u64::from(cert.0.primary_key.created_at().as_secs());
    let own = self_signature(cert).ok_or(KeyRefusal::Invalid)?;
    if expired(created, own, now) {
        return Err(KeyRefusal::Expired);
    }
    if cert.0.version() != KeyVersion::V6
        && !own.features().is_some_and(pgp::packet::Features::seipd_v2)
    {
        return Err(KeyRefusal::NoModernEncryption);
    }
    let mut saw_expired = false;
    let mut saw_weak = false;
    let mut best: Option<(&SignedPublicSubKey, u64)> = None;
    for sub in &cert.0.public_subkeys {
        let revoked = sub
            .signatures
            .iter()
            .any(|s| s.typ() == Some(SignatureType::SubkeyRevocation));
        if revoked {
            continue;
        }
        let bindings = sub
            .signatures
            .iter()
            .filter(|s| s.typ() == Some(SignatureType::SubkeyBinding));
        let Some(binding) = newest(bindings) else {
            continue;
        };
        let flags = binding.key_flags();
        if !flags.encrypt_comms() && !flags.encrypt_storage() {
            continue;
        }
        if !strong(sub.key.public_params()) {
            saw_weak = true;
            continue;
        }
        let sub_created = u64::from(sub.key.created_at().as_secs());
        if expired(sub_created, binding, now) {
            saw_expired = true;
            continue;
        }
        if best.is_none_or(|(_, at)| sub_created >= at) {
            best = Some((sub, sub_created));
        }
    }
    match best {
        Some((sub, _)) => Ok(sub),
        None if saw_expired => Err(KeyRefusal::Expired),
        None if saw_weak => Err(KeyRefusal::Weak),
        None => Err(KeyRefusal::CannotEncrypt),
    }
}
