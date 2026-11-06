//! The size cap and revocations.
//!
//! Pattern: none (tests). Real self-signatures, so the rule meets the packets a real peer could send.

use pgp::packet::{SignatureType, SubpacketData};

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
