//! The device proof: Ed25519, exact signed bytes, keys that are not, single use, enrolment.

mod common;

use common::{AI, Device, FRIEND, OWNER, World};
use fs_messenger_guard::{
    CONTEXT, DeviceVerifier, Ed25519Verifier, Guard, GuardError, GuardProof, Method, Refusal,
    signed_bytes,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn rfc_8032_test_vector_2() {
    let key = unhex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
    let message = unhex("72");
    let signature = unhex(
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
         085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    );
    let v = Ed25519Verifier;
    assert!(v.accepts_key(&key));
    assert!(v.verify(&key, &message, &signature));
    assert!(!v.verify(&key, &unhex("73"), &signature));
}

#[test]
fn a_key_of_low_order_a_malformed_key_and_a_non_canonical_s_are_refused() {
    let v = Ed25519Verifier;
    let identity = unhex("0100000000000000000000000000000000000000000000000000000000000000");
    let order_two = unhex("ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
    let zero = vec![0u8; 32];
    for key in [&identity, &order_two, &zero] {
        assert!(!v.accepts_key(key), "{}", hex(key));
        // The signature that would pass for every message under such a key.
        let mut any = identity.clone();
        any.extend_from_slice(&[0u8; 32]);
        assert!(!v.verify(key, b"anything", &any));
    }
    assert!(!v.accepts_key(&[1, 2, 3]));
    assert!(!v.accepts_key(&[]));

    let d = Device::new(OWNER, 1);
    let good = d.sign(b"m");
    assert!(v.verify(&d.public(), b"m", &good));
    // s + L: the same point arithmetic, and not canonical.
    let l = unhex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    let mut bad = good.clone();
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(bad[32 + i]) + u16::from(l[i]) + carry;
        bad[32 + i] = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    assert!(!v.verify(&d.public(), b"m", &bad));
    assert!(!v.verify(&d.public(), b"m", &good[..63]));
    assert!(!v.verify(&d.public(), b"m", &[]));
}

#[test]
fn the_signed_bytes_are_exactly_these() {
    assert_eq!(CONTEXT, "FreeSynergy.Messenger guard device proof v1");
    let bytes = signed_bytes(
        &[0xee; 2],
        Method::Release,
        "me",
        &[b"ai", b"changes"],
        b"nn",
    )
    .unwrap();
    let mut expected = vec![];
    let field = |out: &mut Vec<u8>, f: &[u8]| {
        out.extend_from_slice(&(f.len() as u32).to_be_bytes());
        out.extend_from_slice(f);
    };
    field(&mut expected, CONTEXT.as_bytes());
    field(&mut expected, &[0xee, 0xee]);
    field(&mut expected, b"release");
    field(&mut expected, b"me");
    expected.extend_from_slice(&2u32.to_be_bytes());
    field(&mut expected, b"ai");
    field(&mut expected, b"changes");
    field(&mut expected, b"nn");
    assert_eq!(bytes, expected);
    // A fixed vector too, so that the helper above cannot drift along with the code.
    assert_eq!(
        hex(&signed_bytes(b"E", Method::Stop, "s", &[], b"N").unwrap()),
        concat!(
            "0000002b",
            "46726565 53796e65 7267792e 4d657373 656e6765 72206775 61726420 64657669 63652070 726f6f66 2076 31",
            "00000001", "45",
            "00000004", "73746f70",
            "00000001", "73",
            "00000000",
            "00000001", "4e",
        )
        .replace(' ', "")
    );
}

#[test]
fn fields_cannot_be_shifted_into_each_other() {
    let sign = |engine: &[u8], signer: &str, arguments: &[&[u8]], nonce: &[u8]| {
        signed_bytes(engine, Method::Release, signer, arguments, nonce).unwrap()
    };
    assert_ne!(
        sign(b"e", "s", &[b"ab", b"c"], b"n"),
        sign(b"e", "s", &[b"a", b"bc"], b"n")
    );
    assert_ne!(
        sign(b"e", "s", &[b"ab"], b"c"),
        sign(b"e", "s", &[b"a"], b"bc")
    );
    assert_ne!(sign(b"e", "sa", &[], b"n"), sign(b"es", "a", &[], b"n"));
    assert_ne!(sign(b"e", "s", &[b""], b"n"), sign(b"e", "s", &[], b"n"));
    assert_ne!(
        sign(b"e", "s", &[b"", b"a"], b"n"),
        sign(b"e", "s", &[b"a", b""], b"n")
    );
}

#[tokio::test]
async fn a_proof_for_another_engine_or_method_or_context_does_not_hold() {
    let w = World::open().await;
    let other_engine = w.owner.proof(&[0x11; 32], Method::Stop, &[]);
    let refused = w.engine.stop(&other_engine).await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    let other_method = w.owner.proof(&w.identity, Method::Reopen, &[]);
    assert!(matches!(
        w.engine.stop(&other_method).await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    let nonce = w.owner.fresh_nonce();
    let mut raw = signed_bytes(&w.identity, Method::Stop, OWNER, &[], &nonce).unwrap();
    raw[4] ^= 1; // the context string
    let proof = GuardProof {
        signer: OWNER.into(),
        nonce,
        proof: w.owner.sign(&raw),
    };
    assert!(matches!(
        w.engine.stop(&proof).await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    assert_eq!(
        w.engine.state().await.mode,
        fs_messenger_guard::GuardMode::Open
    );
}

#[tokio::test]
async fn a_nonce_works_once_also_after_a_restart_and_a_forged_proof_does_not_burn_it() {
    let w = World::open().await;
    let stop = w.stop_proof();
    // Someone guesses the nonce, and has no key.
    let mut forged = stop.clone();
    forged.proof[1] ^= 1;
    assert!(w.engine.stop(&forged).await.is_err());
    assert_eq!(
        w.engine.state().await.mode,
        fs_messenger_guard::GuardMode::Open
    );
    w.engine.stop(&stop).await.unwrap();

    // The real threat: an old "reopen" after a new stop.
    let reopen = w.reopen_proof();
    w.engine.reopen(&reopen).await.unwrap();
    let w = w.restart().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let w = w.restart().await;
    let again = w.engine.reopen(&reopen).await;
    assert!(
        matches!(again, Err(GuardError::Refused(Refusal::Replayed))),
        "{again:?}"
    );
    assert_eq!(
        w.engine.state().await.mode,
        fs_messenger_guard::GuardMode::Stopped
    );
}

#[tokio::test]
async fn a_nonce_is_single_use_across_methods() {
    let w = World::with_ai().await;
    let nonce = w.owner.fresh_nonce();
    let signed = |method: Method, arguments: &[&[u8]]| {
        let bytes = signed_bytes(&w.identity, method, OWNER, arguments, &nonce).unwrap();
        GuardProof {
            signer: OWNER.into(),
            nonce: nonce.clone(),
            proof: w.owner.sign(&bytes),
        }
    };
    let release = signed(Method::Release, &[AI.as_bytes(), b"readsOnly"]);
    let grant = fs_messenger_guard::AiGrant {
        profile: AI.into(),
        risk: fs_messenger_core::ToolCallRisk::ReadsOnly,
    };
    w.engine.release(&grant, &release).await.unwrap();
    let stop = signed(Method::Stop, &[]);
    assert!(matches!(
        w.engine.stop(&stop).await,
        Err(GuardError::Refused(Refusal::Replayed))
    ));
}

#[tokio::test]
async fn a_short_or_empty_nonce_is_refused() {
    let w = World::open().await;
    for nonce in [vec![], vec![1u8; 15], vec![1u8; 65]] {
        let bytes = signed_bytes(&w.identity, Method::Stop, OWNER, &[], &nonce).unwrap();
        let proof = GuardProof {
            signer: OWNER.into(),
            nonce,
            proof: w.owner.sign(&bytes),
        };
        assert!(w.engine.stop(&proof).await.is_err());
    }
    assert_eq!(
        w.engine.state().await.mode,
        fs_messenger_guard::GuardMode::Open
    );
}

#[tokio::test]
async fn enrolment_takes_a_first_key_once_and_a_later_one_only_with_the_owners_proof() {
    let w = World::fresh().await;
    assert!(matches!(
        w.enrolment.enrol_owner(&[0u8; 32]).await,
        Err(GuardError::Refused(Refusal::BadKey))
    ));
    w.enrolment.enrol_owner(&w.owner.public()).await.unwrap();
    let thief = Device::new(OWNER, 66);
    assert!(matches!(
        w.enrolment.enrol_owner(&thief.public()).await,
        Err(GuardError::Refused(Refusal::NotEnrollable(_)))
    ));
    let identity = w.engine.engine_identity().await.unwrap();
    // A key signing for its own addition proves nothing.
    let self_signed = thief.proof(
        &identity,
        Method::AddDevice,
        &[OWNER.as_bytes(), hex(&thief.public()).as_bytes()],
    );
    assert!(matches!(
        w.enrolment
            .add_device(OWNER, &thief.public(), &self_signed)
            .await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    let friend = Device::new(FRIEND, 2);
    let by_owner = w.owner.proof(
        &identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), hex(&friend.public()).as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &by_owner)
        .await
        .unwrap();
    // Stored as public keys only.
    let record = w.record().await.unwrap().to_string();
    assert!(record.contains(&hex(&friend.public())));
    assert!(!record.contains(&hex(w.owner.pair.sk.as_ref())));
}

#[tokio::test]
async fn an_invited_ai_gets_no_device_and_a_removed_key_stops_counting() {
    let w = World::with_ai().await;
    let ai_device = Device::new(AI, 9);
    let proof = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[AI.as_bytes(), hex(&ai_device.public()).as_bytes()],
    );
    assert!(matches!(
        w.enrolment
            .add_device(AI, &ai_device.public(), &proof)
            .await,
        Err(GuardError::Refused(Refusal::IdentityConflict))
    ));

    let second = Device::new(OWNER, 3);
    let key = hex(&second.public());
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[OWNER.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(OWNER, &second.public(), &add)
        .await
        .unwrap();
    let remove = w.owner.proof(
        &w.identity,
        Method::RemoveDevice,
        &[OWNER.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .remove_device(OWNER, &second.public(), &remove)
        .await
        .unwrap();
    assert!(matches!(
        w.engine
            .stop(&second.proof(&w.identity, Method::Stop, &[]))
            .await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
}
