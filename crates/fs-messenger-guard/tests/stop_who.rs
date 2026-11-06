//! Who may pull and open the stop; it needs nothing but the API.

mod common;

use std::sync::Arc;

use common::{AI, Device, FRIEND, OWNER, World};
use fs_messenger_guard::{Gate, Guard, GuardError, GuardMode, Method, Refusal, Verdict};
use serde_json::json;
#[tokio::test]
async fn only_the_owner_may_pull_it_or_open_it_and_not_a_person_with_a_key_of_their_own() {
    let w = World::open().await;
    let friend = Device::new(FRIEND, 2);
    let key: String = friend.public().iter().map(|b| format!("{b:02x}")).collect();
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();

    let refused = w
        .engine
        .stop(&friend.proof(&w.identity, Method::Stop, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    assert_eq!(
        w.engine.state().await.mode,
        GuardMode::Open,
        "no effect at all"
    );

    w.engine.stop(&w.stop_proof()).await.unwrap();
    let refused = w
        .engine
        .reopen(&friend.proof(&w.identity, Method::Reopen, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn an_ai_can_neither_pull_it_nor_open_it_even_with_a_key_and_as_owner_by_a_tampered_record() {
    let w = World::open().await;
    let ai = Device::new(AI, 9);
    let key: String = ai.public().iter().map(|b| format!("{b:02x}")).collect();
    w.invite(AI).await;
    // Someone with the store in hand gives the AI a key.
    w.tamper(|doc| {
        doc["devices"][AI] = json!([{ "keyType": "ed25519", "publicKey": key }]);
    })
    .await;
    let refused = w
        .engine
        .stop(&ai.proof(&w.identity, Method::Stop, &[]))
        .await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let refused = w
        .engine
        .reopen(&ai.proof(&w.identity, Method::Reopen, &[]))
        .await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);

    // Also when it is the owner on paper: being marked AI comes first.
    w.tamper(|doc| {
        doc["invited"][OWNER] = json!({ "by": OWNER, "at": "2026-01-01T00:00:00Z" });
    })
    .await;
    let refused = w.engine.reopen(&w.reopen_proof()).await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn it_works_with_no_messenger_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        fs_store::TursoStore::open_with(dir.path().join("g.db"), common::encrypted())
            .await
            .unwrap(),
    );
    let clock = Arc::new(common::TestClock::default());
    let (engine, enrolment) = common::engine_on(&store, &clock);
    let owner = Device::new(OWNER, 1);
    enrolment.enrol_owner(&owner.public()).await.unwrap();
    let identity = engine.engine_identity().await.unwrap();
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(engine.state().await.mode, GuardMode::Open);

    let report = engine
        .stop(&owner.proof(&identity, Method::Stop, &[]))
        .await
        .unwrap();
    assert!(!report.messenger_attached, "said, not hidden");
    assert_eq!(report.not_persisted, None);
    assert_eq!(engine.state().await.mode, GuardMode::Stopped);
    // And nobody can be asked without a messenger, so nothing gets through that way either.
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    let proof = owner.proof(&identity, Method::Invite, &[AI.as_bytes()]);
    engine.invite(AI, &proof).await.unwrap();
    let verdict = engine
        .check(
            &fs_messenger_guard::Caller::new(AI),
            "message.delete",
            &fs_messenger_guard::Arguments::none(),
        )
        .await;
    assert_eq!(verdict, Verdict::Refuse(Refusal::NoWayToAsk));
}

#[tokio::test]
async fn a_group_stop_answers_unsupported() {
    let w = World::open().await;
    let result = w.engine.stop_group("g", &w.stop_proof()).await;
    assert!(
        matches!(result, Err(GuardError::Unsupported(u)) if u.capability() == "guard.stop.group")
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    use fs_messenger_guard::{OwnerOnly, StopPermission};
    assert!(OwnerOnly.may_stop_group("g", OWNER).is_err());
}
