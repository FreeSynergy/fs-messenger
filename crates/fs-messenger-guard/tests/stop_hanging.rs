//! Blocker: a stop goes first and cannot be held up: not by a store that hangs, a caller that walks
//! away, or a step that panics.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::rigs::{Rig, eventually};
use fs_messenger_core::{Provider, TurnScope, TurnStopOutcome};
use fs_messenger_guard::{Guard, GuardMode, Method, Refusal, Verdict};
use tokio::sync::Notify;
struct Counts(Arc<Notify>);

#[async_trait]
impl Provider for Counts {
    async fn stop_turns(&self, _scope: &TurnScope) -> fs_messenger_core::Result<()> {
        self.0.notify_one();
        Ok(())
    }
}

async fn with_provider(r: &Rig) -> Arc<Notify> {
    let account = r
        .repo
        .add_account(fs_messenger_core::AccountSetup {
            address: "a@x".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    let notified = Arc::new(Notify::new());
    r.repo
        .register_provider(account.id, Arc::new(Counts(notified.clone())));
    notified
}

use fs_messenger_core::Messenger as _;

#[tokio::test]
async fn a_hanging_read_cannot_hold_up_the_stop_the_latch_is_first_and_providers_are_asked() {
    let r = Rig::open().await;
    let asked = with_provider(&r).await;
    let proof = r.proof(Method::Stop);
    r.faulty.hang_loads.store(true, Ordering::SeqCst);
    let engine = r.engine.clone();
    let started = Instant::now();
    let stop = tokio::spawn(async move { engine.stop(&proof).await });
    // Before the read has given up, it refuses already.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    let report = stop.await.unwrap().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    tokio::time::timeout(Duration::from_secs(1), asked.notified())
        .await
        .expect("the provider was asked");
    assert!(report.not_persisted.is_some(), "and it says so");
    assert_eq!(report.turns[0].outcome, TurnStopOutcome::Ended);
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    // When the store comes back, the stop still holds, and goes to disk.
    r.faulty.hang_loads.store(false, Ordering::SeqCst);
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    let faulty = r.faulty.clone();
    eventually("the stop is written", async || {
        use fs_messenger_guard::GuardRepository;
        let held = faulty.real.load().await.unwrap().unwrap();
        held["mode"] == "stopped"
    })
    .await;
}

#[tokio::test]
async fn a_hanging_save_cannot_hold_up_the_stop() {
    let r = Rig::open().await;
    let asked = with_provider(&r).await;
    r.faulty.hang_saves.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let report = r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(report.not_persisted.is_some());
    asked.notified().await;
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn a_caller_that_walks_away_does_not_take_the_stop_with_it() {
    let r = Rig::open().await;
    let asked = with_provider(&r).await;
    let proof = r.proof(Method::Stop);
    r.faulty.hang_saves.store(true, Ordering::SeqCst);
    {
        // The future is dropped at its first wait.
        let stop = r.engine.stop(&proof);
        let dropped = tokio::time::timeout(Duration::from_millis(1), stop).await;
        assert!(dropped.is_err());
    }
    tokio::time::timeout(Duration::from_secs(3), asked.notified())
        .await
        .expect("the provider step went on");
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    r.faulty.hang_saves.store(false, Ordering::SeqCst);
    let faulty = r.faulty.clone();
    eventually("the stop is written", async || {
        use fs_messenger_guard::GuardRepository;
        faulty.real.load().await.unwrap().unwrap()["mode"] == "stopped"
    })
    .await;
}

#[tokio::test]
async fn a_panic_in_one_step_takes_no_other_step_with_it() {
    let r = Rig::open().await;
    let asked = with_provider(&r).await;
    r.faulty.panic_saves.store(true, Ordering::SeqCst);
    r.faulty.panic_appends.store(true, Ordering::SeqCst);
    let report = r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    assert!(report.not_persisted.is_some());
    assert!(report.not_logged.is_some());
    asked.notified().await;
    assert_eq!(report.turns.len(), 1);
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
}

#[tokio::test]
async fn a_check_on_a_latched_engine_does_not_wait_for_the_store() {
    let r = Rig::open().await;
    r.faulty.hang_saves.store(true, Ordering::SeqCst);
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    r.faulty.hang_loads.store(true, Ordering::SeqCst);
    let started = Instant::now();
    for _ in 0..20 {
        assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
    }
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_bad_proof_with_a_readable_state_takes_the_latch_back_and_no_answer_does_not() {
    let r = Rig::open().await;
    let mut bad = r.proof(Method::Stop);
    bad.proof[0] ^= 1;
    assert!(r.engine.stop(&bad).await.is_err());
    assert!(matches!(r.read_check().await, Verdict::Allow(_)));
    assert_eq!(r.engine.state().await.mode, GuardMode::Open);

    // The same bad proof while the store does not answer: it cannot be shown bad, so it stays shut.
    r.faulty.hang_loads.store(true, Ordering::SeqCst);
    let _ = r.engine.stop(&bad).await;
    r.faulty.hang_loads.store(false, Ordering::SeqCst);
    assert_eq!(r.read_check().await, Verdict::Refuse(Refusal::Stopped));
}
