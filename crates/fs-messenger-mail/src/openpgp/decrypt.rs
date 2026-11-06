//! Decrypting and verifying: [`decrypt`], [`verify_detached`], [`SignatureState`].
//!
//! Pattern: Facade over rPGP's message reader. An OpenPGP message that is not encrypted is an error
//! here, so a sender cannot put a plain literal packet into an encrypted-looking envelope and have it
//! counted as decrypted. Reading to the end checks integrity (AEAD or MDC): tampered data fails.

use std::io::{BufReader, Cursor};

use pgp::composed::{
    Deserializable, DetachedSignature, Message, SignedPublicKey, SignedPublicSubKey,
};
use pgp::types::{Password, VerifyingKey};

use super::{PublicCert, SecretCert};
use crate::error::{MailError, Result};

/// What the signature on a message says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SignatureState {
    /// No signature.
    #[default]
    NotSigned,
    /// It verifies with this certificate, which was offered as the sender's.
    Valid {
        /// Fingerprint of the certificate.
        fingerprint: String,
    },
    /// It is signed, but with no certificate known for the sender, or it does not verify.
    Unverified,
}

/// The keys of a certificate that may sign: the primary key and every subkey.
fn verifiers(cert: &SignedPublicKey) -> Vec<&dyn VerifyingKey> {
    let mut keys: Vec<&dyn VerifyingKey> = vec![&cert.primary_key];
    keys.extend(cert.public_subkeys.iter().map(|s| s as &dyn VerifyingKey));
    keys
}

/// Decrypt with `own` and check the signature against `signers`. Fails if `own` opens nothing.
pub fn decrypt(
    armored: &[u8],
    own: &[SecretCert],
    signers: &[PublicCert],
) -> Result<(Vec<u8>, SignatureState), MailError> {
    if own.is_empty() {
        return Err(MailError::Pgp("no own key".into()));
    }
    let (message, _) = Message::from_armor(BufReader::new(Cursor::new(armored)))?;
    if !message.is_encrypted() {
        return Err(MailError::Pgp("the OpenPGP message is not encrypted".into()));
    }
    let empty = Password::empty();
    let passwords = own.iter().map(|_| &empty).collect();
    let keys = own.iter().map(|k| &k.0).collect();
    let mut message = message.decrypt_with_keys(passwords, keys)?;
    if message.is_compressed() {
        message = message.decompress()?;
    }
    let plaintext = message
        .as_data_vec()
        .map_err(|e| MailError::Pgp(e.to_string()))?;
    let state = if message.is_signed() {
        signers
            .iter()
            .find(|cert| {
                verifiers(&cert.0)
                    .into_iter()
                    .any(|k| message.verify(k).is_ok())
            })
            .map_or(SignatureState::Unverified, |cert| SignatureState::Valid {
                fingerprint: cert.fingerprint(),
            })
    } else {
        SignatureState::NotSigned
    };
    Ok((plaintext, state))
}

/// Check a detached signature (RFC 3156 section 5) over `content`.
pub fn verify_detached(armored: &[u8], content: &[u8], signers: &[PublicCert]) -> SignatureState {
    let Ok((signature, _)) = DetachedSignature::from_armor_single(Cursor::new(armored)) else {
        return SignatureState::Unverified;
    };
    signers
        .iter()
        .find(|cert| {
            let by = |k: &SignedPublicSubKey| signature.signature.verify(k, Cursor::new(content));
            signature
                .signature
                .verify(&cert.0.primary_key, Cursor::new(content))
                .is_ok()
                || cert.0.public_subkeys.iter().any(|s| by(s).is_ok())
        })
        .map_or(SignatureState::Unverified, |cert| SignatureState::Valid {
            fingerprint: cert.fingerprint(),
        })
}

