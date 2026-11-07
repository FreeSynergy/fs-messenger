//! The size cap and revocations.
//!
//! Pattern: none (tests). Real self-signatures, so the rule meets the packets a real peer could send.

use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
use pgp::packet::{KeyFlags, Signature, SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::types::{KeyDetails, KeyVersion, Password, Timestamp};

use super::merge::compact_binding;
use super::usable_tests::direct;
use super::*;

/// A real change that is no revocation, so the size is the only reason left to refuse it.
#[test]
fn a_merge_past_the_cap_is_refused() {
    let key = generate_key("a@x").unwrap();
    let held = key.public();
    let mut other = key.public();
    let mut flags = pgp::packet::KeyFlags::default();
    flags.set_sign(true);
    flags.set_certify(true);
    let newer = direct(
        &key,
        SignatureType::Key,
        vec![SubpacketData::KeyFlags(flags)],
    );
    other.0.details.direct_signatures.push(newer);
    let size = held.to_binary().unwrap().len();
    assert!(
        held.merged_within(&other, 64 * 1024).is_some(),
        "it is a real change"
    );
    assert!(
        held.merged_within(&other, size).is_none(),
        "and past the cap it is refused"
    );
}

/// A revocation never fails on the cap: a full certificate must not stay usable after its owner revoked it.
#[test]
fn a_revocation_goes_in_whatever_the_cap_says() {
    let key = generate_key("a@x").unwrap();
    let held = key.public();
    let mut other = key.public();
    other
        .0
        .details
        .revocation_signatures
        .push(direct(&key, SignatureType::KeyRevocation, vec![]));
    let merged = held.merged_within(&other, 1).expect("past any cap");
    assert_eq!(
        usable_encryption_key(&merged, unix_now()).err(),
        Some(KeyRefusal::Revoked)
    );
}

/// A revocation and something else in one form, past the cap: the revocation stays, the rest does not.
#[test]
fn past_the_cap_only_the_revocation_is_taken() {
    let key = generate_key("a@x").unwrap();
    let held = key.public();
    let mut other = key.public();
    other
        .0
        .details
        .revocation_signatures
        .push(direct(&key, SignatureType::KeyRevocation, vec![]));
    let newer = direct(&key, SignatureType::Key, vec![]);
    other.0.details.direct_signatures.push(newer);
    let merged = held.merged_within(&other, 1).expect("the revocation");
    assert_eq!(merged.0.details.revocation_signatures.len(), 1);
    assert_eq!(
        merged.0.details.direct_signatures.len(),
        held.0.details.direct_signatures.len()
    );
}

/// Only the owner can write revocations, and even so they are bounded.
#[test]
fn revocations_are_bounded() {
    let key = generate_key("a@x").unwrap();
    let mut held = key.public();
    for round in 0..3 {
        let mut other = key.public();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            other.0.details.revocation_signatures.push(direct(
                &key,
                SignatureType::KeyRevocation,
                vec![],
            ));
        }
        if let Some(merged) = held.merged_with(&other) {
            held = merged;
        }
        assert!(
            held.0.details.revocation_signatures.len() <= 16,
            "round {round}"
        );
    }
}

/// A key with one signing subkey.
fn signing_key() -> SecretCert {
    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_encrypt(EncryptionCaps::None)
        .can_sign(true)
        .build()
        .unwrap();
    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v2(true)
        .primary_user_id("<a@x>".to_string())
        .subkey(subkey)
        .build()
        .unwrap();
    SecretCert(params.generate(rand::thread_rng()).unwrap())
}

/// A signature on the subkey by its owner, with `embedded` in the unhashed area.
fn on_subkey(key: &SecretCert, typ: SignatureType, signs: bool, embedded: Signature) -> Signature {
    let primary = &key.0.primary_key;
    let mut config = SignatureConfig::from_key(rand::thread_rng(), primary, typ).unwrap();
    let mut hashed = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::now())).unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    if signs {
        let mut flags = KeyFlags::default();
        flags.set_sign(true);
        hashed.push(Subpacket::regular(SubpacketData::KeyFlags(flags)).unwrap());
    }
    config.hashed_subpackets = hashed;
    config.unhashed_subpackets =
        vec![Subpacket::regular(SubpacketData::EmbeddedSignature(Box::new(embedded))).unwrap()];
    let public = key.public();
    let subkey = &public.0.public_subkeys[0].key;
    config
        .sign_subkey_binding(primary, primary.public_key(), &Password::empty(), subkey)
        .unwrap()
}

fn real_back(key: &SecretCert) -> Signature {
    key.0.secret_subkeys[0]
        .key
        .sign_primary_key_binding(
            rand::thread_rng(),
            key.0.primary_key.public_key(),
            &Password::empty(),
        )
        .unwrap()
}

fn kept(key: &SecretCert, signature: &Signature) -> usize {
    let public = key.public();
    let compact = compact_binding(
        key.0.primary_key.public_key(),
        &public.0.public_subkeys[0].key,
        signature,
    )
    .unwrap();
    compact.config().unwrap().unhashed_subpackets.len()
}

#[test]
fn a_real_back_signature_stays_on_a_sign_flagged_binding() {
    let key = signing_key();
    let binding = on_subkey(&key, SignatureType::SubkeyBinding, true, real_back(&key));
    assert_eq!(kept(&key, &binding), 1);
}

/// A public back-signature can be copied. On anything but a sign-flagged binding it has no place, even
/// though it verifies.
#[test]
fn a_real_back_signature_goes_from_anything_else() {
    let key = signing_key();
    let revocation = on_subkey(
        &key,
        SignatureType::SubkeyRevocation,
        false,
        real_back(&key),
    );
    assert_eq!(kept(&key, &revocation), 0, "on a revocation");
    let unflagged = on_subkey(&key, SignatureType::SubkeyBinding, false, real_back(&key));
    assert_eq!(kept(&key, &unflagged), 0, "on a binding that cannot sign");
}

/// An embedded signature that does not verify goes, also where a back-signature would belong.
#[test]
fn an_embedded_signature_that_does_not_verify_goes() {
    let (key, other) = (signing_key(), signing_key());
    let binding = on_subkey(&key, SignatureType::SubkeyBinding, true, real_back(&other));
    assert_eq!(kept(&key, &binding), 0);
}
