//! When in doubt it stays shut: an unreadable, missing or corrupt state reads as stopped, and a stop
//! that cannot be written still stops. Real store underneath; only the failure of a write is injected.

mod common;

use std::sync::Arc;

use common::{AI, World, args};
use fs_messenger_core::ToolCallRisk;
use fs_messenger_guard::{
    Guard, GuardError, GuardMode, Method, Refusal, STATE_ID, TursoGuardRepository, Verdict,
};
use fs_store::traits::UnloggedObjects;
use fs_store::{Envelope, ObjectId, StoredObject, TursoStore};
use serde_json::{Value, json};
async fn stopped_after(w: &World) {
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert!(w.engine.state().await.grants.is_empty());
    assert!(w.engine.state().await.invited.is_empty());
}

#[tokio::test]
async fn each_kind_of_corrupt_record_reads_as_stopped_also_after_a_restart() {
    let corrupt: Vec<(&str, Value)> = vec![
        ("not an object", json!("oops")),
        ("empty", json!({})),
        ("null", Value::Null),
        ("a newer version", json!({ "version": 99 })),
    ];
    for (name, value) in corrupt {
        let w = World::working().await;
        w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
        w.tamper(|doc| *doc = value.clone()).await;
        assert!(w.engine.damage().await.is_some(), "{name}");
        stopped_after(&w).await;
        let w = w.restart().await;
        stopped_after(&w).await;
    }
}

#[tokio::test]
async fn a_record_that_lost_one_field_reads_as_stopped() {
    for field in [
        "mode",
        "engine",
        "devices",
        "invited",
        "grants",
        "calls",
        "usedProofs",
        "version",
    ] {
        let w = World::working().await;
        w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
        w.tamper(|doc| {
            doc.as_object_mut().unwrap().remove(field);
        })
        .await;
        stopped_after(&w).await;
    }
}

#[tokio::test]
async fn an_unrecognised_mode_reads_as_stopped() {
    let w = World::working().await;
    w.tamper(|doc| doc["mode"] = json!("wideOpen")).await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
}

#[tokio::test]
async fn a_deleted_state_reads_as_stopped_like_a_fresh_program() {
    let w = World::working().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    w.store.delete_unlogged(id).await.unwrap();
    stopped_after(&w).await;
    assert!(w.engine.damage().await.is_none(), "it is simply not there");
    let w = w.restart().await;
    stopped_after(&w).await;
}

#[tokio::test]
async fn an_object_of_another_type_under_the_id_reads_as_stopped() {
    let w = World::working().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    let mut good = w.store.read_unlogged(id).await.unwrap().unwrap();
    let content = good.content.clone();
    good.envelope.object_type = "Note".into();
    good.content = content;
    w.store.put_unlogged(good).await.unwrap();
    stopped_after(&w).await;
}

#[tokio::test]
async fn while_damaged_nothing_can_be_signed_and_the_stop_says_so() {
    let w = World::working().await;
    let (stop, reopen) = (w.stop_proof(), w.reopen_proof());
    w.tamper(|doc| *doc = json!("oops")).await;
    let report = w.engine.stop(&stop).await.unwrap();
    assert!(report.not_persisted.is_some(), "said, not hidden");
    assert!(matches!(
        w.engine.reopen(&reopen).await,
        Err(GuardError::Refused(Refusal::Damaged))
    ));
    assert!(matches!(
        w.enrolment.enrol_owner(&w.owner.public()).await,
        Err(GuardError::Refused(Refusal::Damaged))
    ));
}

#[tokio::test]
async fn recovery_is_local_starts_stopped_and_kills_every_older_proof() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let old_reopen = w.reopen_proof();
    let old_identity = w.identity.clone();
    // Not on a healthy state.
    assert!(matches!(
        w.enrolment.recover_damaged(&w.owner.public()).await,
        Err(GuardError::Refused(Refusal::NotEnrollable(_)))
    ));
    w.tamper(|doc| *doc = json!("oops")).await;
    w.enrolment
        .recover_damaged(&w.owner.public())
        .await
        .unwrap();
    stopped_after(&w).await;
    let identity = w.engine.engine_identity().await.unwrap();
    assert_ne!(identity, old_identity);
    assert!(matches!(
        w.engine.reopen(&old_reopen).await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    w.engine
        .reopen(&w.owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    assert!(w.engine.state().await.grants.is_empty());
}

#[tokio::test]
async fn the_repository_refuses_a_store_that_is_not_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Arc::new(TursoStore::open(dir.path().join("p.db")).await.unwrap());
    assert!(TursoGuardRepository::open(plain).is_err());
    let memory = Arc::new(TursoStore::open_in_memory().await.unwrap());
    assert!(TursoGuardRepository::open(memory).is_err());
}

#[tokio::test]
async fn the_state_object_is_marked_sensitive() {
    let w = World::open().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    let object: StoredObject = w.store.read_unlogged(id).await.unwrap().unwrap();
    assert!(!object.envelope.sensitive.is_empty());
    let _ = Envelope::new("x", "y");
}

#[tokio::test]
async fn a_whole_record_of_a_newer_version_reads_as_stopped() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    w.tamper(|doc| doc["version"] = json!(2)).await;
    assert!(w.engine.damage().await.is_some());
    stopped_after(&w).await;
}

#[tokio::test]
async fn every_recovery_has_an_identity_of_its_own_so_no_proof_outlives_one() {
    let w = World::open().await;
    w.tamper(|doc| *doc = json!("oops")).await;
    w.enrolment
        .recover_damaged(&w.owner.public())
        .await
        .unwrap();
    let first = w.engine.engine_identity().await.unwrap();
    assert_eq!(first.len(), 32);
    let old = w.owner.proof(&first, Method::Reopen, &[]);
    w.tamper(|doc| *doc = json!("oops")).await;
    w.enrolment
        .recover_damaged(&w.owner.public())
        .await
        .unwrap();
    assert_ne!(w.engine.engine_identity().await.unwrap(), first);
    assert!(matches!(
        w.engine.reopen(&old).await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
}
