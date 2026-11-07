//! Signing and encrypting one payload.
//!
//! Pattern: Facade over rPGP's message builder, Strategy by [`Method`]. RFC 3156 section 6.2, the
//! combined form: one OpenPGP message that is signed and encrypted, no compression. [`Method::Modern`]
//! is a v2 SEIPD with AES-256 and OCB and v6 PKESK packets; [`Method::Older`] is a v1 SEIPD with AES-256
//! and an MDC and v3 PKESK packets. The two never mix in one message (RFC 9580 sections 5.1.1 and
//! 5.1.2): the builder's type decides the PKESK version, so there is no way to write the illegal pair.
//! Hidden recipients get an anonymous session key packet, so that the others cannot read their key id
//! from the message.

use pgp::composed::{ArmorOptions, MessageBuilder, SignedPublicSubKey};
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::crypto::{aead::AeadAlgorithm, aead::ChunkSize, hash::HashAlgorithm};
use pgp::types::Password;

use super::SecretCert;
use crate::error::{MailError, Result};

/// How a message is encrypted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `SEIPDv2` with `PKESKv6` (RFC 9580).
    Modern,
    /// `SEIPDv1` with an MDC and `PKESKv3` (RFC 4880, still in RFC 9580): for programs that read nothing
    /// newer, `GnuPG` 2.4 and RNP 0.18 among them.
    Older,
}

/// Who can read the message. At least the sender's own key must be among `visible`.
pub struct EncryptionTargets<'a> {
    /// `to`, `cc` and the sender.
    pub visible: Vec<&'a SignedPublicSubKey>,
    /// `bcc`.
    pub hidden: Vec<&'a SignedPublicSubKey>,
}

/// One armored OpenPGP message. The signer's key version decides the signature's: a v4 key for
/// [`Method::Older`], so that the programs which read nothing newer can check it.
pub fn sign_and_encrypt(
    plaintext: &[u8],
    signer: &SecretCert,
    targets: &EncryptionTargets<'_>,
    method: Method,
) -> Result<String, MailError> {
    if targets.visible.is_empty() {
        return Err(MailError::Pgp("nobody to encrypt to".into()));
    }
    let mut rng = rand::thread_rng();
    let source = MessageBuilder::from_bytes("", plaintext.to_vec());
    let hash = HashAlgorithm::Sha512;
    let armor = ArmorOptions::default();
    match method {
        Method::Modern => {
            let chunk = ChunkSize::default();
            let aead = AeadAlgorithm::Ocb;
            let mut b = source.seipd_v2(&mut rng, SymmetricKeyAlgorithm::AES256, aead, chunk);
            for key in &targets.visible {
                b.encrypt_to_key(&mut rng, *key)?;
            }
            for key in &targets.hidden {
                b.encrypt_to_key_anonymous(&mut rng, *key)?;
            }
            b.sign(&signer.0.primary_key, Password::empty(), hash);
            Ok(b.to_armored_string(&mut rng, armor)?)
        }
        Method::Older => {
            let mut b = source.seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
            for key in &targets.visible {
                b.encrypt_to_key(&mut rng, *key)?;
            }
            for key in &targets.hidden {
                b.encrypt_to_key_anonymous(&mut rng, *key)?;
            }
            b.sign(&signer.0.primary_key, Password::empty(), hash);
            Ok(b.to_armored_string(&mut rng, armor)?)
        }
    }
}
