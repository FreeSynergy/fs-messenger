//! A save that panics in the retry task of a stop does not end the retries: once saving works again,
//! the stop is written, with nobody calling anything.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::World;
use common::rigs::{Rig, eventually};
use fs_messenger_guard::{Guard, GuardMode, GuardRepository, Method};

#[tokio::test]
async fn a_save_that_panics_does_not_end_the_retries() {
    let r = Rig::open().await;
    r.faulty.panic_saves.store(true, Ordering::SeqCst);
    r.engine
        .stop(&r.owner.proof(&r.identity, Method::Stop, &[]))
        .await
        .unwrap();
    // Let the retry task run into the panic a few times.
    tokio::time::sleep(Duration::from_millis(300)).await;
    r.faulty.panic_saves.store(false, Ordering::SeqCst);
    assert_eq!(r.engine.state().await.mode, GuardMode::Stopped);

    let real = r.faulty.clone();
    eventually("the stop is written after the panics", async || {
        World::doc_mode(&real.real.load().await.unwrap().unwrap()) == "stopped"
    })
    .await;
    assert!(!r.engine.stop_unwritten());
}
