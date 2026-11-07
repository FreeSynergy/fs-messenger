//! Which keys are refused: expired, revoked, unable to encrypt.
//!
//! Pattern: none (tests). The bad keys are made with real self-signatures, so the rule meets the packets
//! a real peer could send.

use pgp::composed::{KeyType, SecretKeyParamsBuilder};
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::types::{Duration, KeyDetails, KeyVersion, Password, Timestamp};

use super::*;

pub(super) fn direct(
    key: &SecretCert,
    typ: SignatureType,
    extra: Vec<SubpacketData>,
) -> pgp::packet::Signature {
    let primary = &key.0.primary_key;
    let mut config = SignatureConfig::from_key(rand::thread_rng(), primary, typ).unwrap();
    let mut hashed = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::now())).unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    hashed.extend(extra.into_iter().map(|d| Subpacket::regular(d).unwrap()));
    config.hashed_subpackets = hashed;
    config
        .sign_key(primary, &Password::empty(), primary.public_key())
        .unwrap()
}

#[test]
fn a_new_key_is_usable_and_a_round_trip_through_bytes_keeps_it_so() {
    let key = generate_key("a@x").unwrap().public();
    assert!(usable_encryption_key(&key, unix_now()).is_ok());
    let again = PublicCert::from_binary(&key.to_binary().unwrap()).unwrap();
    assert_eq!(again.fingerprint(), key.fingerprint());
    assert!(usable_encryption_key(&again, unix_now()).is_ok());
}

#[test]
fn a_revoked_key_is_refused() {
    let key = generate_key("a@x").unwrap();
    let revocation = direct(&key, SignatureType::KeyRevocation, vec![]);
    let mut public = key.public();
    public.0.details.revocation_signatures.push(revocation);
    let bytes = public.to_binary().unwrap();
    let parsed = PublicCert::from_binary(&bytes).unwrap();
    assert_eq!(
        usable_encryption_key(&parsed, unix_now()).err(),
        Some(KeyRefusal::Revoked)
    );
}

#[test]
fn an_expired_key_is_refused_and_was_fine_before() {
    let key = generate_key("a@x").unwrap();
    let life = SubpacketData::KeyExpirationTime(Duration::from_secs(3600));
    let newer = direct(&key, SignatureType::Key, vec![life]);
    let mut public = key.public();
    public.0.details.direct_signatures.push(newer);
    let parsed = PublicCert::from_binary(&public.to_binary().unwrap()).unwrap();
    let created = u64::from(parsed.0.primary_key.created_at().as_secs());
    assert!(usable_encryption_key(&parsed, created + 10).is_ok());
    assert_eq!(
        usable_encryption_key(&parsed, created + 7200).err(),
        Some(KeyRefusal::Expired)
    );
}

#[test]
fn a_key_without_an_encryption_subkey_is_refused() {
    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v1(false)
        .feature_seipd_v2(true)
        .primary_user_id("<a@x>".to_string())
        .build()
        .unwrap();
    let key = SecretCert(params.generate(rand::thread_rng()).unwrap());
    assert_eq!(
        usable_encryption_key(&key.public(), unix_now()).err(),
        Some(KeyRefusal::CannotEncrypt)
    );
}

#[test]
fn a_signature_that_does_not_verify_is_refused() {
    let (a, b) = (generate_key("a@x").unwrap(), generate_key("b@x").unwrap());
    let mut public = a.public();
    // A subkey of someone else's, under a's primary key: the binding cannot verify.
    public.0.public_subkeys = b.public().0.public_subkeys.clone();
    assert_eq!(
        usable_encryption_key(&public, unix_now()).err(),
        Some(KeyRefusal::Invalid)
    );
}

pub(super) fn known(cert: &PublicCert) -> crate::keys::PeerKey {
    crate::keys::PeerKey::new("a@x", cert, crate::keys::KeySource::AutocryptFirstSeen, 1).unwrap()
}

pub(super) fn offer(cert: &PublicCert) -> crate::autocrypt::Offer {
    crate::autocrypt::Offer {
        cert: cert.clone(),
        proven_by_known_key: false,
    }
}

/// The owner's revocation counts even in an unproven header: nobody else can make one. And a forger who
/// replays the older form cannot take it back.
#[test]
fn a_revocation_is_taken_and_never_given_up() {
    use crate::autocrypt::{Decision, decide};
    let key = generate_key("a@x").unwrap();
    let live = key.public();
    let mut revoked = key.public();
    let revocation = direct(&key, SignatureType::KeyRevocation, vec![]);
    revoked.0.details.revocation_signatures.push(revocation);
    let now = unix_now();
    assert_eq!(
        decide(Some(&known(&live)), &offer(&revoked), now),
        Decision::SameKey { take_offered: true }
    );
    assert_eq!(
        decide(Some(&known(&revoked)), &offer(&live), now),
        Decision::SameKey {
            take_offered: false
        }
    );
    // Neither is a revoked key a first contact, or a replacement.
    assert_eq!(decide(None, &offer(&revoked), now), Decision::Ignore);
    let other = generate_key("a@x").unwrap().public();
    assert_eq!(
        decide(Some(&known(&other)), &offer(&revoked), now),
        Decision::Ignore
    );
}
