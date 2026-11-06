//! Whether a certificate may be encrypted to now: [`usable_encryption_key`].
//!
//! Pattern: Specification. rPGP gives the packets, not the semantics, so the rule is written here:
//! every self-signature must verify, the key must not be revoked or expired, and a live encryption
//! subkey must exist. Provisional: a subkey revocation fails rPGP's binding check, so such a whole
//! certificate is refused as invalid instead of falling back to another subkey; a key that encrypts
//! only with its primary key is refused too. A v4 key that does not advertise SEIPDv2 is refused:
//! SEIPDv1 is `Unsupported` outgoing.

use pgp::composed::SignedPublicSubKey;
use pgp::packet::{Signature, SignatureType};
use pgp::types::{KeyDetails, KeyVersion};

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
    /// A v4 key that does not advertise SEIPDv2.
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
    let lifetime = sig.key_expiration_time().map_or(0, |d| u64::from(d.as_secs()));
    lifetime != 0 && created + lifetime <= now
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
    let created = u64::from(cert.0.primary_key.created_at().as_secs());
    let own = self_signature(cert).ok_or(KeyRefusal::Invalid)?;
    if expired(created, own, now) {
        return Err(KeyRefusal::Expired);
    }
    if cert.0.version() != KeyVersion::V6 && !own.features().is_some_and(|f| f.seipd_v2()) {
        return Err(KeyRefusal::NoModernEncryption);
    }
    let mut saw_expired = false;
    let mut best: Option<(&SignedPublicSubKey, u64)> = None;
    for sub in &cert.0.public_subkeys {
        let bindings = sub
            .signatures
            .iter()
            .filter(|s| s.typ() == Some(SignatureType::SubkeyBinding));
        let Some(binding) = newest(bindings) else { continue };
        let flags = binding.key_flags();
        if !flags.encrypt_comms() && !flags.encrypt_storage() {
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
        None => Err(KeyRefusal::CannotEncrypt),
    }
}
