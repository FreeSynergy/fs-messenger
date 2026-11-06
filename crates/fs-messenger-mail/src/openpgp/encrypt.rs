//! Signing and encrypting one payload.
//!
//! Pattern: Facade over rPGP's message builder. RFC 3156 section 6.2, the combined form: one OpenPGP
//! message that is signed and encrypted. SEIPDv2 with AES-256 and OCB (RFC 9580), no compression.
//! Hidden recipients get an anonymous session key packet, so that the others cannot read their key id
//! from the message.

use pgp::composed::{ArmorOptions, MessageBuilder, SignedPublicSubKey};
use pgp::crypto::{aead::AeadAlgorithm, aead::ChunkSize, hash::HashAlgorithm};
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::Password;

use super::SecretCert;
use crate::error::{MailError, Result};

/// Who can read the message. At least the sender's own key must be among `visible`.
pub struct EncryptionTargets<'a> {
    /// `to`, `cc` and the sender.
    pub visible: Vec<&'a SignedPublicSubKey>,
    /// `bcc`.
    pub hidden: Vec<&'a SignedPublicSubKey>,
}

/// One armored OpenPGP message.
pub fn sign_and_encrypt(
    plaintext: &[u8],
    signer: &SecretCert,
    targets: &EncryptionTargets<'_>,
) -> Result<String, MailError> {
    if targets.visible.is_empty() {
        return Err(MailError::Pgp("nobody to encrypt to".into()));
    }
    let mut rng = rand::thread_rng();
    let mut builder = MessageBuilder::from_bytes("", plaintext.to_vec()).seipd_v2(
        &mut rng,
        SymmetricKeyAlgorithm::AES256,
        AeadAlgorithm::Ocb,
        ChunkSize::default(),
    );
    for key in &targets.visible {
        builder.encrypt_to_key(&mut rng, *key)?;
    }
    for key in &targets.hidden {
        builder.encrypt_to_key_anonymous(&mut rng, *key)?;
    }
    builder.sign(&signer.0.primary_key, Password::empty(), HashAlgorithm::Sha512);
    Ok(builder.to_armored_string(&mut rng, ArmorOptions::default())?)
}
