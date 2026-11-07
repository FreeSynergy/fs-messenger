//! Encrypting and opening with real keys, no server.
//!
//! Pattern: none (tests): every case runs real rPGP; nothing is a double.

use super::*;
use crate::error::MailError;

const LIMIT: usize = 1 << 20;

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
    sign_and_encrypt(b"the secret text", signer, &targets, Method::Modern).unwrap()
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
        LIMIT,
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
    assert!(decrypt(armored.as_bytes(), &[alice], &[], LIMIT).is_ok());
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
    let (_, state) = decrypt(armored.as_bytes(), &[bob], &[alice.public()], LIMIT)?;
    assert_eq!(state, SignatureState::Unverified);
    Ok(())
}

#[test]
fn a_hidden_recipient_can_read_it_and_only_their_key_id_is_hidden() -> Result<(), MailError> {
    let (alice, bob, hidden) = (
        generate_key("a@x")?,
        generate_key("b@x")?,
        generate_key("h@x")?,
    );
    let armored = encrypt_to(&alice, &[&bob.public()], &[&hidden.public()]);
    // What the others can read: one named recipient, and one packet that names nobody. The packet is
    // there to be counted, so THAT someone is hidden, and how many, stays visible.
    let (message, _) = pgp::composed::Message::from_armor(armored.as_bytes())?;
    let pgp::composed::Message::Encrypted { esk, .. } = &message else {
        panic!("encrypted");
    };
    let named: Vec<bool> = esk
        .iter()
        .map(|e| match e {
            pgp::composed::Esk::PublicKeyEncryptedSessionKey(
                pgp::packet::PublicKeyEncryptedSessionKey::V6 { fingerprint, .. },
            ) => fingerprint.is_some(),
            other => panic!("a v6 packet, not {other:?}"),
        })
        .collect();
    assert_eq!(named, [true, false]);
    let (text, _) = decrypt(armored.as_bytes(), &[hidden], &[], LIMIT)?;
    assert_eq!(text, b"the secret text");
    assert!(decrypt(armored.as_bytes(), &[bob], &[], LIMIT).is_ok());
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
    assert!(decrypt(armored.as_bytes(), &[eve], &[], LIMIT).is_err());
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
    assert!(decrypt(changed.join("\n").as_bytes(), &[bob], &[], LIMIT).is_err());
    Ok(())
}

#[test]
fn a_literal_message_is_not_decrypted_whatever_its_envelope_says() -> Result<(), MailError> {
    use pgp::composed::{ArmorOptions, MessageBuilder};
    let bob = generate_key("b@x")?;
    let literal = MessageBuilder::from_bytes("", b"not encrypted at all".to_vec())
        .to_armored_string(rand::thread_rng(), ArmorOptions::default())?;
    assert!(decrypt(literal.as_bytes(), &[bob], &[], LIMIT).is_err());
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

#[test]
fn a_plaintext_over_the_limit_is_refused_without_being_held() -> Result<(), MailError> {
    let (alice, bob) = (generate_key("a@x")?, generate_key("b@x")?);
    let armored = encrypt_to(&alice, &[&bob.public()], &[]);
    let text = b"the secret text".len();
    let keys = std::slice::from_ref(&bob);
    assert!(
        decrypt(armored.as_bytes(), keys, &[], text).is_ok(),
        "exactly the limit is fine"
    );
    assert!(decrypt(armored.as_bytes(), keys, &[], text - 1).is_err());
    Ok(())
}

/// The whole point of `decrypt_selecting`: it opens once, and shows the plaintext before it verifies.
#[test]
fn the_signer_is_chosen_after_reading_the_plaintext() -> Result<(), MailError> {
    let (alice, bob) = (generate_key("a@x")?, generate_key("b@x")?);
    let armored = encrypt_to(&alice, &[&bob.public()], &[]);
    let mut calls = 0;
    let (_, state) = decrypt_selecting(armored.as_bytes(), &[bob], LIMIT, |text| {
        calls += 1;
        assert_eq!(text, b"the secret text");
        Ok(vec![alice.public()])
    })?;
    assert_eq!(calls, 1);
    assert!(matches!(state, SignatureState::Valid { .. }));
    Ok(())
}
