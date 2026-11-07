//! Blocker: a disk that does not answer cannot hold the engine's lock for ever. A `reopen` that must
//! put the marker back, because a stop came in while it removed it, gives up after the same time limit
//! as every other step; the stop stays in force. The stall is made by filling the runtime's one thread
//! for blocking work, which is where all marker file work runs.

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::rigs::Rig;
use fs_messenger_guard::{Guard, GuardMode, Method, Refusal, Verdict};

async fn stall_then_reopen_with_a_stop_in_between() {
    let r = std::sync::Arc::new(Rig::open().await);
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    let (release, held) = mpsc::channel::<()>();
    let disk = tokio::task::spawn_blocking(move || {
        let _ = held.recv();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (engine, reopen) = (r.engine.clone(), r.proof(Method::Reopen));
    let reopening = tokio::spawn(async move { engine.reopen(&reopen).await });
    // The reopen waits on the disk to remove the marker; a stop comes in meanwhile.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let local = r.clone();
    let stopping = tokio::spawn(async move { local.enrolment.stop_now().await });

    let ended = tokio::time::timeout(Duration::from_secs(2), reopening).await;
    let _ = release.send(());
    disk.await.unwrap();
    let ended = ended.expect("a reopen does not wait on the disk for ever");
    assert!(
        matches!(
            ended.unwrap(),
            Err(fs_messenger_guard::GuardError::Refused(Refusal::Stopped))
        ),
        "the stop that came in wins"
    );
    let _ = stopping.await;
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);
    assert!(matches!(
        r.read_check().await,
        Verdict::Refuse(Refusal::Stopped)
    ));
}

#[test]
fn a_reopen_that_puts_the_marker_back_does_not_hold_the_lock_on_a_stalled_disk() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(stall_then_reopen_with_a_stop_in_between());
}
