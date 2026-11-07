//! A stop that cannot be written still stops, and says so. Real store underneath; only the failure of a
//! write or read is injected.

mod common;

use std::sync::atomic::Ordering;

use common::rigs::{Rig, eventually};
use common::{AI, World, args};
use fs_messenger_core::{MessageAction, Messenger};
use fs_messenger_guard::{
    Arguments, Caller, Gate, Guard, GuardEngine, GuardError, GuardMode, GuardRepository, Method,
    Refusal, STATE_ID, Verdict,
};
use fs_objects::ObjectId;
use fs_objects::traits::UnloggedObjects;
use serde_json::json;
async fn verdict(engine: &GuardEngine) -> Verdict {
    let input = Arguments::from_value(&args("/")).unwrap();
    engine.check(&Caller::new(AI), "message.open", &input).await
}

#[tokio::test]
async fn a_stop_that_cannot_be_written_still_stops_and_says_so() {
    let r = Rig::open().await;
    assert!(matches!(verdict(&r.engine).await, Verdict::Allow(_)));
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    let report = r
        .engine
        .stop(&r.owner.proof(&r.identity, Method::Stop, &[]))
        .await
        .unwrap();
    assert!(
        report
            .not_persisted
            .as_deref()
            .unwrap()
            .contains("disk full")
    );
    assert!(report.not_logged.is_some());
    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
    assert!(
        r.engine.state().await.grants.is_empty(),
        "taken in memory too"
    );
    // The store still says open: this is the case the report warns about.
    assert_eq!(
        World::doc_mode(&r.faulty.real.load().await.unwrap().unwrap()),
        "open"
    );

    // Once writing works again it goes to disk, with nobody calling anything.
    r.faulty.fail_writes.store(false, Ordering::SeqCst);
    let real = r.faulty.clone();
    eventually("the stop is written", async || {
        World::doc_mode(&real.real.load().await.unwrap().unwrap()) == "stopped"
    })
    .await;
    assert_eq!(
        real.real.load().await.unwrap().unwrap()["grants"],
        json!([])
    );
    assert!(!r.engine.stop_unwritten());
    drop(r.dir);
    drop(r.store);
}

#[tokio::test]
async fn a_reopen_that_cannot_be_written_stays_shut() {
    let r = Rig::open().await;
    r.engine
        .stop(&r.owner.proof(&r.identity, Method::Stop, &[]))
        .await
        .unwrap();
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    let result = r
        .engine
        .reopen(&r.owner.proof(&r.identity, Method::Reopen, &[]))
        .await;
    assert!(matches!(result, Err(GuardError::NotPersisted(_))));
    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn a_reopen_after_a_stop_that_was_not_written_does_not_bring_the_releases_back() {
    let r = Rig::open().await;
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    r.engine
        .stop(&r.owner.proof(&r.identity, Method::Stop, &[]))
        .await
        .unwrap();
    r.faulty.fail_writes.store(false, Ordering::SeqCst);
    r.engine
        .reopen(&r.owner.proof(&r.identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert!(r.engine.state().await.grants.is_empty());
    assert!(!matches!(verdict(&r.engine).await, Verdict::Allow(_)));
}

#[tokio::test]
async fn when_the_store_stops_answering_everything_is_refused_and_comes_back_only_as_it_was() {
    let r = Rig::open().await;
    r.faulty.fail_reads.store(true, Ordering::SeqCst);
    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
    assert!(r.engine.damage().await.is_some());
    let enrolment_like = r.engine.engine_identity().await;
    assert!(enrolment_like.is_none());
    r.faulty.fail_reads.store(false, Ordering::SeqCst);
    assert!(matches!(verdict(&r.engine).await, Verdict::Allow(_)));
}

#[tokio::test]
async fn an_allow_once_that_cannot_be_used_up_in_the_state_is_refused() {
    // Over a real messenger: the question and the decision go the normal way.
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    // The store holds the object; now a logged write has given the id a history, so the unlogged
    // write of the state fails for real. It reads, and it cannot be written.
    let id: ObjectId = STATE_ID.parse().unwrap();
    let current = w.store.read_unlogged(id).await.unwrap().unwrap();
    use fs_objects::Store;
    w.store.put(current, "someone").await.unwrap();
    assert_eq!(
        w.check("message.delete", args("/m")).await,
        Verdict::Refuse(Refusal::NotRecorded)
    );
}

#[tokio::test]
async fn a_stop_during_a_reopen_that_is_being_written_wins() {
    let r = Rig::open().await;
    r.engine
        .stop(&r.owner.proof(&r.identity, Method::Stop, &[]))
        .await
        .unwrap();
    // A reopen is at its write, holding the lock. Now the owner pulls the stop again.
    r.faulty.hold_save.store(true, Ordering::SeqCst);
    let (engine, reopen) = (
        r.engine.clone(),
        r.owner.proof(&r.identity, Method::Reopen, &[]),
    );
    let reopening = tokio::spawn(async move { engine.reopen(&reopen).await });
    r.faulty.entered.notified().await;
    let (engine, stop) = (
        r.engine.clone(),
        r.owner.proof(&r.identity, Method::Stop, &[]),
    );
    let stopping = tokio::spawn(async move { engine.stop(&stop).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    r.faulty.go.notify_one();
    assert!(
        reopening.await.unwrap().is_err(),
        "a reopen that a stop overtook says so"
    );
    stopping.await.unwrap().unwrap();

    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    let record = r.faulty.real.load().await.unwrap().unwrap();
    assert_eq!(
        World::doc_mode(&record),
        "stopped",
        "and it stays so after a restart"
    );
}
