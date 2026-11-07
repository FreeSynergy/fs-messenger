//! Subkeys, and a revoked held key: changes to one key that a forger cannot make.
//!
//! Pattern: none (tests). Real self-signatures, so the rule meets the packets a real peer could send.

use pgp::composed::{KeyType, SecretKeyParamsBuilder};
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::types::{KeyDetails, KeyVersion, Password, Timestamp};

use super::usable::may_vouch;
use super::usable_tests::{direct, known, offer};
use super::*;

fn subkey_signature(
    key: &SecretCert,
    typ: SignatureType,
    subkey: &pgp::packet::PublicSubkey,
    at: u32,
    encrypts: bool,
) -> pgp::packet::Signature {
    let primary = &key.0.primary_key;
    let mut config = SignatureConfig::from_key(rand::thread_rng(), primary, typ).unwrap();
    let mut hashed = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::from_secs(
            at,
        )))
        .unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    if encrypts {
        let mut flags = pgp::packet::KeyFlags::default();
        flags.set_encrypt_comms(true);
        hashed.push(Subpacket::regular(SubpacketData::KeyFlags(flags)).unwrap());
    }
    config.hashed_subpackets = hashed;
    config
        .sign_subkey_binding(primary, primary.public_key(), &Password::empty(), subkey)
        .unwrap()
}

fn now_u32() -> u32 {
    u32::try_from(unix_now()).unwrap()
}

/// The certificate of `key` with its only encryption subkey revoked by its owner.
fn with_subkey_revoked(key: &SecretCert) -> PublicCert {
    let mut public = key.public();
    let sub = public.0.public_subkeys[0].key.clone();
    let revocation = subkey_signature(
        key,
        SignatureType::SubkeyRevocation,
        &sub,
        now_u32() + 5,
        false,
    );
    public.0.public_subkeys[0].signatures.push(revocation);
    PublicCert::from_binary(&public.to_binary().unwrap()).unwrap()
}

#[test]
fn a_revoked_subkey_is_not_encrypted_to() {
    let key = generate_key("a@x").unwrap();
    let revoked = with_subkey_revoked(&key);
    assert_eq!(
        usable_encryption_key(&revoked, unix_now()).err(),
        Some(KeyRefusal::CannotEncrypt)
    );
}

#[test]
fn a_revoked_subkey_falls_back_to_a_live_one() {
    let key = generate_key("a@x").unwrap();
    let other = generate_key("b@x").unwrap().public();
    let mut public = with_subkey_revoked(&key);
    let spare = other.0.public_subkeys[0].key.clone();
    let binding = subkey_signature(&key, SignatureType::SubkeyBinding, &spare, now_u32(), true);
    let fingerprint = spare.fingerprint();
    public
        .0
        .public_subkeys
        .push(pgp::composed::SignedPublicSubKey::new(spare, vec![binding]));
    let chosen = usable_encryption_key(&public, unix_now()).unwrap();
    assert_eq!(chosen.key.fingerprint(), fingerprint);
}

/// Findings 2 and 9: a revoked subkey and a new subkey are changes to the same key, and an older form
/// without the revocation cannot take it back.
#[test]
fn subkey_changes_count_for_the_same_key() {
    use crate::autocrypt::{Decision, decide};
    let key = generate_key("a@x").unwrap();
    let live = key.public();
    let revoked = with_subkey_revoked(&key);
    let now = unix_now();
    let taken = Decision::SameKey { take_offered: true };
    let kept = Decision::SameKey {
        take_offered: false,
    };
    assert_eq!(decide(Some(&known(&live)), &offer(&revoked), now), taken);
    assert_eq!(decide(Some(&known(&revoked)), &offer(&live), now), kept);

    let other = generate_key("b@x").unwrap().public();
    let mut bigger = key.public();
    let spare = other.0.public_subkeys[0].key.clone();
    let binding = subkey_signature(
        &key,
        SignatureType::SubkeyBinding,
        &spare,
        now_u32() + 9,
        true,
    );
    bigger
        .0
        .public_subkeys
        .push(pgp::composed::SignedPublicSubKey::new(spare, vec![binding]));
    assert_eq!(
        decide(Some(&known(&live)), &offer(&bigger), now),
        taken,
        "a new subkey is learned"
    );
    assert_eq!(decide(Some(&known(&bigger)), &offer(&live), now), kept);
}

/// A held key that is revoked proves nothing, however the mail was signed.
#[test]
fn a_revoked_held_key_proves_no_rotation() {
    use crate::autocrypt::{Decision, Offer, decide};
    let key = generate_key("a@x").unwrap();
    let mut held = key.public();
    held.0
        .details
        .revocation_signatures
        .push(direct(&key, SignatureType::KeyRevocation, vec![]));
    let next = generate_key("a@x").unwrap().public();
    let proof = Offer {
        cert: next,
        companion: None,
        proven_by_known_key: true,
    };
    assert_eq!(
        decide(Some(&known(&held)), &proof, unix_now()),
        Decision::Conflict
    );
    assert!(!may_vouch(&held));
    assert!(may_vouch(&key.public()));
}

#[test]
fn dsa_is_refused_however_well_formed() {
    use pgp::composed::{DsaKeySize, EncryptionCaps, SubkeyParamsBuilder};
    use pgp::crypto::ecc_curve::ECCCurve;
    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
        .can_encrypt(EncryptionCaps::All)
        .build()
        .unwrap();
    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::Dsa(DsaKeySize::B1024))
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v2(true)
        .primary_user_id("<a@x>".to_string())
        .subkey(subkey)
        .build()
        .unwrap();
    let key = SecretCert(params.generate(rand::thread_rng()).unwrap());
    assert_eq!(
        usable_encryption_key(&key.public(), unix_now()).err(),
        Some(KeyRefusal::Weak)
    );
    assert!(!may_vouch(&key.public()));
}

/// A signature made after the key ran out proves nothing. The flag has to be on the newest self-signature.
#[test]
fn a_signature_made_after_the_key_ran_out_is_not_valid() {
    use pgp::composed::{ArmorOptions, DetachedSignature};
    use pgp::crypto::hash::HashAlgorithm;
    let key = generate_key("a@x").unwrap();
    let mut flags = pgp::packet::KeyFlags::default();
    flags.set_sign(true);
    flags.set_certify(true);
    let lived = direct(
        &key,
        SignatureType::Key,
        vec![
            SubpacketData::KeyFlags(flags),
            SubpacketData::KeyExpirationTime(pgp::types::Duration::from_secs(1)),
        ],
    );
    let mut public = key.public();
    public.0.details.direct_signatures.push(lived);
    std::thread::sleep(std::time::Duration::from_millis(2100));
    let armored = DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &key.0.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha512,
        &b"content"[..],
    )
    .unwrap()
    .to_armored_bytes(ArmorOptions::default())
    .unwrap();
    assert_eq!(
        verify_detached(&armored, b"content", std::slice::from_ref(&public)),
        SignatureState::Unverified,
    );
    assert!(matches!(
        verify_detached(&armored, b"content", &[key.public()]),
        SignatureState::Valid { .. }
    ));
}
