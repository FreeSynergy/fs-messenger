//! Blocker: nobody is left without a stop, and no key removal opens enrolment again.

mod common;

use common::{AI, Device, FRIEND, OWNER, World, args};
use fs_messenger_core::{ToolCallOutcome, ToolCallRisk, ToolCallState};
use fs_messenger_guard::{Guard, GuardError, GuardMode, Method, Refusal, Verdict};

#[tokio::test]
async fn a_person_without_a_working_key_can_stop_on_the_local_path() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let q = w.ask("message.delete", args("/m")).await;
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Allow(_)
    ));

    // The device is lost: any proof it could make fails.
    let lost = Device::new(OWNER, 66);
    let refused = w
        .engine
        .stop(&lost.proof(&w.identity, Method::Stop, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::BadProof))
    ));

    let report = w.enrolment.stop_now().await.unwrap();
    assert_eq!(report.not_persisted, None);
    assert_eq!(report.calls_cancelled, 1);
    let state = w.engine.state().await;
    assert_eq!(state.mode, GuardMode::Stopped);
    assert!(state.grants.is_empty());
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    let shown = w.shown(&q).await;
    assert_eq!(shown.state, ToolCallState::Settled);
    assert_eq!(shown.outcome, ToolCallOutcome::Cancelled);
    assert_eq!(w.engine.log().await.unwrap().last().unwrap().event, "stop");

    let w = w.restart().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn the_local_stop_cannot_open_and_reopen_always_needs_a_key() {
    let w = World::open().await;
    w.enrolment.stop_now().await.unwrap();
    let lost = Device::new(OWNER, 66);
    let refused = w
        .engine
        .reopen(&lost.proof(&w.identity, Method::Reopen, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
}

#[tokio::test]
async fn the_local_stop_works_on_a_program_that_has_no_state_yet() {
    let w = World::fresh().await;
    let report = w.enrolment.stop_now().await.unwrap();
    assert_eq!(report.not_persisted, None);
    w.enrolment.enrol_owner(&w.owner.public()).await.unwrap();
    let identity = w.engine.engine_identity().await.unwrap();
    w.engine
        .reopen(&w.owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
}

#[tokio::test]
async fn the_owners_last_key_cannot_be_removed_and_enrolment_stays_shut() {
    let w = World::open().await;
    let key: String = w
        .owner
        .public()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let proof = w.owner.proof(
        &w.identity,
        Method::RemoveDevice,
        &[OWNER.as_bytes(), key.as_bytes()],
    );
    let before = w.record().await;
    let refused = w
        .enrolment
        .remove_device(OWNER, &w.owner.public(), &proof)
        .await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::LastOwnerKey))),
        "{refused:?}"
    );
    assert_eq!(w.record().await, before);
    // So nobody gets to put in a "first" key.
    let thief = Device::new(OWNER, 66);
    assert!(w.enrolment.enrol_owner(&thief.public()).await.is_err());
}

#[tokio::test]
async fn another_persons_last_key_can_go() {
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
    let remove = w.owner.proof(
        &w.identity,
        Method::RemoveDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .remove_device(FRIEND, &friend.public(), &remove)
        .await
        .unwrap();
}
