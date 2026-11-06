//! Creating our own key.
//!
//! Pattern: Builder (rPGP's): an RFC 9580 v6 key, Ed25519 primary for certifying and signing, X25519
//! subkey for encrypting, advertising SEIPDv2 only. No passphrase: the secret rests only in an
//! encrypted store (ADR 0024, 0029). Provisional: no expiry; rotation is `Unsupported` as a flow.

use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
use pgp::crypto::{aead::AeadAlgorithm, hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::types::KeyVersion;

use super::SecretCert;
use crate::error::{MailError, Result};


/// A new key for `address`.
pub fn generate_key(address: &str) -> Result<SecretCert, MailError> {
    let bad = |e: &dyn std::fmt::Display| MailError::Pgp(e.to_string());
    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::X25519)
        .can_encrypt(EncryptionCaps::All)
        .build()
        .map_err(|e| bad(&e))?;
    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v1(false)
        .feature_seipd_v2(true)
        .primary_user_id(format!("<{address}>"))
        .preferred_symmetric_algorithms(
            [SymmetricKeyAlgorithm::AES256, SymmetricKeyAlgorithm::AES128]
                .into_iter()
                .collect(),
        )
        .preferred_hash_algorithms(
            [HashAlgorithm::Sha512, HashAlgorithm::Sha256]
                .into_iter()
                .collect(),
        )
        .preferred_aead_algorithms(
            [
                (SymmetricKeyAlgorithm::AES256, AeadAlgorithm::Ocb),
                (SymmetricKeyAlgorithm::AES128, AeadAlgorithm::Ocb),
            ]
            .into_iter()
            .collect(),
        )
        .subkey(subkey)
        .build()
        .map_err(|e| bad(&e))?;
    Ok(SecretCert(params.generate(rand::thread_rng())?))
}
