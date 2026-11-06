//! Encrypting and opening with real keys, no server.
//!
//! Pattern: none (tests): every case runs real rPGP; nothing is a double.

use super::*;
use crate::error::MailError;

fn encrypt_to(signer: &SecretCert, visible: &[&PublicCert], hidden: &[&PublicCert]) -> String {
    let now = unix_now();
    let targets = EncryptionTargets {
        visible: visible
            .iter()
            .map(|c| usable_encryption_key(c, now).unwrap())
            .collect(),
        hidden: hidden
            .iter()
            .map(|c| usable_encryption_key(c, now).unwrap())
            .collect(),
    };
    sign_and_encrypt(b"the secret text", signer, &targets).unwrap()
}

#[test]
fn a_signed_mail_opens_and_verifies() -> Result<(), MailError> {
    let (alice, bob) = (generate_key("a@x")?, generate_key("b@x")?);
    let armored = encrypt_to(&alice, &[&bob.public(), &alice.public()], &[]);
    assert!(!armored.contains("secret"));
    let (text, state) = decrypt(
        armored.as_bytes(),
        std::slice::from_ref(&bob),
        &[alice.public()],
    )?;
    assert_eq!(text, b"the secret text");
    assert_eq!(
        state,
        SignatureState::Valid {
            fingerprint: alice.fingerprint()
        }
    );
    Ok(())
}

#[test]
fn the_sender_can_read_his_own_copy() -> Result<(), MailError> {
    let (alice, bob) = (generate_key("a@x")?, generate_key("b@x")?);
    let armored = encrypt_to(&alice, &[&bob.public(), &alice.public()], &[]);
    assert!(decrypt(armored.as_bytes(), &[alice], &[]).is_ok());
    Ok(())
}

#[test]
fn a_signature_by_someone_else_is_not_valid() -> Result<(), MailError> {
    let (alice, bob, eve) = (
        generate_key("a@x")?,
        generate_key("b@x")?,
        generate_key("e@x")?,
    );
    let armored = encrypt_to(&eve, &[&bob.public()], &[]);
    let (_, state) = decrypt(armored.as_bytes(), &[bob], &[alice.public()])?;
    assert_eq!(state, SignatureState::Unverified);
    Ok(())
}

#[test]
fn a_hidden_recipient_can_read_it_and_others_cannot_tell() -> Result<(), MailError> {
    let (alice, bob, hidden) = (
        generate_key("a@x")?,
        generate_key("b@x")?,
        generate_key("h@x")?,
    );
    let armored = encrypt_to(&alice, &[&bob.public()], &[&hidden.public()]);
    let (text, _) = decrypt(armored.as_bytes(), &[hidden], &[])?;
    assert_eq!(text, b"the secret text");
    assert!(decrypt(armored.as_bytes(), &[bob], &[]).is_ok());
    Ok(())
}

#[test]
fn whoever_is_not_addressed_opens_nothing() -> Result<(), MailError> {
    let (alice, bob, eve) = (
        generate_key("a@x")?,
        generate_key("b@x")?,
        generate_key("e@x")?,
    );
    let armored = encrypt_to(&alice, &[&bob.public()], &[]);
    assert!(decrypt(armored.as_bytes(), &[eve], &[]).is_err());
    Ok(())
}

#[test]
fn tampered_data_does_not_open() -> Result<(), MailError> {
    let (alice, bob) = (generate_key("a@x")?, generate_key("b@x")?);
    let armored = encrypt_to(&alice, &[&bob.public()], &[]);
    // Flip a character in the middle of the base64 body.
    let lines: Vec<&str> = armored.lines().collect();
    let mid = lines.len() / 2;
    let flipped: String = lines[mid].chars().rev().collect();
    let mut changed: Vec<&str> = lines.clone();
    changed[mid] = &flipped;
    assert!(decrypt(changed.join("\n").as_bytes(), &[bob], &[]).is_err());
    Ok(())
}

#[test]
fn a_literal_message_is_not_decrypted_whatever_its_envelope_says() -> Result<(), MailError> {
    use pgp::composed::{ArmorOptions, MessageBuilder};
    let bob = generate_key("b@x")?;
    let literal = MessageBuilder::from_bytes("", b"not encrypted at all".to_vec())
        .to_armored_string(rand::thread_rng(), ArmorOptions::default())?;
    assert!(decrypt(literal.as_bytes(), &[bob], &[]).is_err());
    Ok(())
}

#[test]
fn a_detached_signature_verifies_only_over_its_content() -> Result<(), MailError> {
    use pgp::composed::{ArmorOptions, DetachedSignature};
    use pgp::crypto::hash::HashAlgorithm;
    use pgp::types::Password;
    let alice = generate_key("a@x")?;
    let signature = DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &alice.0.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha512,
        &b"content"[..],
    )?;
    let armored = signature.to_armored_bytes(ArmorOptions::default())?;
    let good = verify_detached(&armored, b"content", &[alice.public()]);
    assert_eq!(
        good,
        SignatureState::Valid {
            fingerprint: alice.fingerprint()
        }
    );
    assert_eq!(
        verify_detached(&armored, b"changed", &[alice.public()]),
        SignatureState::Unverified
    );
    Ok(())
}
