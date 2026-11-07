//! A stop that comes in while a reopen is under way wins: latch, mark and marker stay, and no way in
//! lets anything through in between. Slow stores are made with held loads, saves and log entries.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::rigs::{Rig, eventually};
use common::{AI, World};
use fs_messenger_core::{AccountSetup, Ingest, Ingested, Messenger as _};
use fs_messenger_guard::{Guard, GuardError, GuardRepository as _, Method, Refusal, Verdict};

async fn stopped_rig() -> Rig {
    let r = Rig::open().await;
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    r
}

#[tokio::test]
async fn a_stop_behind_a_slow_reopen_is_not_lost_when_the_wait_for_the_lock_ends() {
    let r = stopped_rig().await;
    // A holds the lock at a slow read; the reopen waits behind it, then reads and writes slowly too.
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    let (engine, invite) = (
        r.engine.clone(),
        r.owner.proof(&r.identity, Method::Invite, &[b"x"]),
    );
    let busy = tokio::spawn(async move { engine.invite("x", &invite).await });
    r.faulty.entered.notified().await;
    let (engine, reopen) = (r.engine.clone(), r.proof(Method::Reopen));
    let reopening = tokio::spawn(async move { engine.reopen(&reopen).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (engine, stop) = (r.engine.clone(), r.proof(Method::Stop));
    let stopping = tokio::spawn(async move { engine.stop(&stop).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    // Each step of the reopen takes 250 ms: together more than the stop waits for the lock (600 ms).
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    r.faulty.hold_save.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(210)).await;
    r.faulty.go.notify_one();
    r.faulty.entered.notified().await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    r.faulty.go.notify_one();
    r.faulty.entered.notified().await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    r.faulty.go.notify_one();

    let _ = busy.await.unwrap();
    let reopened = reopening.await.unwrap();
    let report = stopping.await.unwrap().unwrap();
    assert!(
        matches!(reopened, Err(GuardError::Refused(Refusal::Stopped))),
        "{reopened:?} {report:?}"
    );
    assert_eq!(
        r.read_check().await,
        Verdict::Refuse(Refusal::Stopped),
        "{report:?}"
    );
    assert!(r.dir.path().join("guard.stopped").exists(), "the marker");
    eventually("the stop is written", async || !r.engine.stop_unwritten()).await;
    let record = r.faulty.real.load().await.unwrap().unwrap();
    assert_eq!(World::doc_mode(&record), "stopped");
}

#[tokio::test]
async fn nothing_gets_in_between_the_end_of_a_reopen_and_the_stop_that_came_during_it() {
    let r = stopped_rig().await;
    let agent = r
        .repo
        .add_account(AccountSetup {
            address: "agents@local".into(),
            kind: "agent".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    r.faulty.hold_save.store(true, Ordering::SeqCst);
    let (engine, reopen) = (r.engine.clone(), r.proof(Method::Reopen));
    let reopening = tokio::spawn(async move { engine.reopen(&reopen).await });
    r.faulty.entered.notified().await;
    let (engine, stop) = (r.engine.clone(), r.proof(Method::Stop));
    let stopping = tokio::spawn(async move { engine.stop(&stop).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The reopen goes on to its log entry, where it waits: the stop has not been written yet.
    r.faulty.hold_append.store(true, Ordering::SeqCst);
    r.faulty.go.notify_one();
    r.faulty.entered.notified().await;
    let mut raw = fs_messenger_core::RawMessage::new(
        fs_messenger_core::ExternalId::new("uid", "1"),
        fs_messenger_core::ExternalId::new("thread", "1"),
        fs_messenger_core::Kind::Chat,
        AI,
        "hello",
    );
    raw.encryption = fs_messenger_core::Encryption::Mls;
    let way_in = r.repo.ingest(agent.id, raw).await;
    r.faulty.go.notify_one();
    let _ = reopening.await.unwrap();
    stopping.await.unwrap().unwrap();
    assert!(
        !matches!(way_in, Ok(Ingested::Stored(_))),
        "an agent message got in after a stop: {way_in:?}"
    );
}
