//! Device proofs at the engine: other engines and methods, single use, enrolment.

mod common;

use common::{AI, Device, FRIEND, OWNER, World};
use fs_messenger_guard::{Guard, GuardError, GuardProof, Method, Refusal, signed_bytes};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
