//! The retry task of a stop dies with the runtime it was started on. The next stop or `check` on
//! another runtime has to start a new one: the flag that says "a task is running" must not outlive it.

mod common;

use std::sync::atomic::Ordering;

use common::World;
use common::rigs::{Rig, eventually};
use fs_messenger_guard::{Guard, GuardRepository, Method};

#[tokio::test]
async fn a_retry_task_lost_with_its_runtime_is_started_again() {
    let r = Rig::open().await;
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    let engine = r.engine.clone();
    let proof = r.owner.proof(&r.identity, Method::Stop, &[]);
    // A helper runtime: the stop is pulled on it, the retry task starts there, and it ends.
    std::thread::spawn(move || {
        let helper = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        helper.block_on(async { engine.stop(&proof).await.unwrap() });
        drop(helper);
    })
    .join()
    .unwrap();
    assert!(r.engine.stop_unwritten(), "the write failed, so it is open");

    r.faulty.fail_writes.store(false, Ordering::SeqCst);
    // Anyone asking on this runtime starts the retries again.
    let _ = r.read_check().await;
    let real = r.faulty.clone();
    eventually("the stop is written by a new retry task", async || {
        World::doc_mode(&real.real.load().await.unwrap().unwrap()) == "stopped"
    })
    .await;
    assert!(!r.engine.stop_unwritten());
}
