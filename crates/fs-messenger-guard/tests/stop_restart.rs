//! Blocker: a stop that could not be written survives a restart through its marker, with its used
//! proof, and something keeps trying to write it. What remains if both fail is a stated limit.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::rigs::{Faulty, Rig, config_in, engine_over, eventually};
use common::{AI, World};
use fs_messenger_guard::{
    Gate, Guard, GuardError, GuardMode, GuardRepository, Method, Refusal, Verdict,
};
use fs_store::TursoStore;

/// As after a restart: only the store's files and the marker stay.
async fn restart_on(r: Rig) -> (Arc<fs_messenger_guard::GuardEngine>, Arc<Faulty>, Rig2) {
    let Rig {
        dir,
        store,
        faulty,
        repo,
        engine,
        enrolment,
        clock,
        owner,
        identity,
    } = r;
    drop((repo, engine, enrolment, faulty));
    let faulty = loop {
        // A retry in flight can hold the last handle for a moment.
        match fs_messenger_guard::TursoGuardRepository::open(store.clone()) {
            Ok(real) => break Faulty::wrap(real),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    };
    let (engine, enrolment) = engine_over(faulty.clone(), dir.path(), clock);
    (
        engine,
        faulty,
        Rig2 {
            dir,
            store,
            enrolment,
            owner,
            identity,
        },
    )
}

struct Rig2 {
    dir: tempfile::TempDir,
    store: Arc<TursoStore>,
    enrolment: fs_messenger_guard::LocalEnrolment,
    owner: common::Device,
    identity: Vec<u8>,
}

#[tokio::test]
async fn an_unwritten_stop_survives_a_restart_with_nothing_coming_back() {
    let r = Rig::open().await;
    assert!(matches!(r.read_check().await, Verdict::Allow(_)));
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    let stop = r.proof(Method::Stop);
    let report = r.engine.stop(&stop).await.unwrap();
    assert!(report.not_persisted.is_some());
    assert_eq!(report.not_marked, None, "the marker went to disk");
    // The store still holds the old, open state.
    assert_eq!(
        World::doc_mode(&r.faulty.real.load().await.unwrap().unwrap()),
        "open"
    );

    let (engine, faulty, rest) = restart_on(r).await;
    assert_eq!(engine.state().await.mode, GuardMode::Stopped);
    assert!(engine.state().await.grants.is_empty());
    let input = fs_messenger_guard::Arguments::from_text(r#"{"path":"/"}"#).unwrap();
    let caller = fs_messenger_guard::Caller::new(AI);
    assert_eq!(
        engine.check(&caller, "message.open", &input).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    // The used proof came along: it does not replay.
    assert!(matches!(
        engine.stop(&stop).await,
        Err(GuardError::Refused(Refusal::Replayed))
    ));
    // And now it reaches the store, by itself.
    let held = faulty.clone();
    eventually("the marked stop is written", async || {
        World::doc_mode(&held.real.load().await.unwrap().unwrap()) == "stopped"
    })
    .await;
    assert_eq!(
        held.real.load().await.unwrap().unwrap()["grants"],
        serde_json::json!([])
    );
    // Only a written reopen takes the marker away.
    let marker = config_in(rest.dir.path()).stop_marker;
    assert!(marker.exists());
    let reopen = rest.owner.proof(&rest.identity, Method::Reopen, &[]);
    engine.reopen(&reopen).await.unwrap();
    assert!(!marker.exists());
    drop(rest.enrolment);
    drop(rest.store);
}

#[tokio::test]
async fn an_approved_destructive_call_does_not_come_back_after_a_restart() {
    use fs_messenger_core::{MessageAction, Messenger};
    let w = World::with_ai().await;
    let q = w.ask("message.delete", common::args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    // Stop written; then the store is put back to what it held before, as a lost write would.
    let open_record = w.record().await.unwrap();
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.tamper(|doc| *doc = open_record.clone()).await;
    let w = w.restart().await;
    assert_eq!(
        w.check("message.delete", common::args("/m")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn a_marker_with_unreadable_content_is_still_a_stop() {
    let w = World::open().await;
    let marker = w.dir.path().join("guard.stopped");
    let w = {
        std::fs::write(&marker, b"not json").unwrap();
        w.restart().await
    };
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert!(!marker.exists());
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    let w = w.restart().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
}

#[tokio::test]
async fn a_written_stop_keeps_its_marker_and_a_failed_reopen_too() {
    let r = Rig::open().await;
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    let marker = config_in(r.dir.path()).stop_marker;
    assert!(marker.exists(), "until a written reopen");
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    assert!(r.engine.reopen(&r.proof(Method::Reopen)).await.is_err());
    assert!(marker.exists());
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn any_write_that_carries_the_stop_clears_it_and_one_that_does_not_cannot() {
    let r = Rig::open().await;
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    let stop = r.proof(Method::Stop);
    r.engine.stop(&stop).await.unwrap();
    assert!(r.engine.stop_unwritten());
    r.faulty.fail_writes.store(false, Ordering::SeqCst);
    let faulty = r.faulty.clone();
    let engine = r.engine.clone();
    eventually("it is written", async || !engine.stop_unwritten()).await;
    let record = faulty.real.load().await.unwrap().unwrap();
    assert_eq!(World::doc_mode(&record), "stopped");
    // What was written holds the used proof too: it does not replay, whatever memory says.
    r.engine.reopen(&r.proof(Method::Reopen)).await.unwrap();
    assert!(matches!(
        r.engine.stop(&stop).await,
        Err(GuardError::Refused(Refusal::Replayed))
    ));
}

#[tokio::test]
async fn reopen_goes_by_the_latch_when_the_record_says_open() {
    let w = World::with_ai().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.tamper(|doc| doc["mode"] = serde_json::json!("open"))
        .await;
    assert_eq!(
        w.engine.state().await.mode,
        GuardMode::Stopped,
        "the latch holds"
    );
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    assert!(matches!(
        w.check("message.open", common::args("/")).await,
        Verdict::Ask(_)
    ));
    assert!(matches!(
        w.engine.reopen(&w.reopen_proof()).await,
        Err(GuardError::Refused(Refusal::AlreadyOpen))
    ));
}
