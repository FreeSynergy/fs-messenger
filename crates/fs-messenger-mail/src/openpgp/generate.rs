//! Creating our own keys.
//!
//! Pattern: Builder (rPGP's). [`generate_key`]: an RFC 9580 v6 key, Ed25519 primary for certifying and
//! signing, X25519 subkey for encrypting, advertising `SEIPDv2` only. [`generate_legacy_key`]: a v4 key
//! in the legacy forms (`EdDSA` with the Ed25519 curve, ECDH with Curve25519), advertising `SEIPDv1`,
//! because `GnuPG` and RNP read nothing newer (`DESIGN.md`, "Interoperability"). RFC 9580 section 5.5.2
//! says an implementation SHOULD NOT generate a v4 key; the owner decided to do it all the same
//! (ADR 0056, point 9), to reach people whose program has no v6. No passphrase: the secret rests only in
//! an encrypted store (ADR 0024, 0029). Provisional: no expiry; rotation is `Unsupported` as a flow.

use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
use pgp::crypto::ecc_curve::ECCCurve;
use pgp::crypto::{aead::AeadAlgorithm, hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::types::KeyVersion;

use super::SecretCert;
use crate::error::{MailError, Result};

fn bad(e: &dyn std::fmt::Display) -> MailError {
    MailError::Pgp(e.to_string())
}

/// A new v6 key for `address`.
pub fn generate_key(address: &str) -> Result<SecretCert, MailError> {
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

/// A new v4 key for `address`, in the form Autocrypt 1.1 asks for: Ed25519 (`EdDSA`, algorithm 22) to
/// certify and sign, Curve25519 (ECDH, algorithm 18) to encrypt, one user id.
pub fn generate_legacy_key(address: &str) -> Result<SecretCert, MailError> {
    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
        .can_encrypt(EncryptionCaps::All)
        .build()
        .map_err(|e| bad(&e))?;
    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v1(true)
        .feature_seipd_v2(false)
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
        .subkey(subkey)
        .build()
        .map_err(|e| bad(&e))?;
    Ok(SecretCert(params.generate(rand::thread_rng())?))
}
