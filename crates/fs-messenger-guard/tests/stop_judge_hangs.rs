//! A verifier or a permission that never answers cannot hold a stop up for good, nor lose it: after
//! the time limit the stop is one by nobody, with marker and retry as for any stop.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::rigs::{Faulty, config_in};
use common::{Device, OWNER, TestClock, encrypted};
use fs_messenger_guard::{
    DeviceVerifier, Ed25519Verifier, Guard, GuardEngine, GuardMode, Method, OwnerOnly,
    StopPermission,
};
use fs_objects::TursoStore;

/// Ed25519, but it waits first for as long as the flag is set.
struct Blocks(AtomicBool);

impl DeviceVerifier for Blocks {
    fn key_type(&self) -> &'static str {
        Ed25519Verifier.key_type()
    }
    fn accepts_key(&self, key: &[u8]) -> bool {
        Ed25519Verifier.accepts_key(key)
    }
    fn verify(&self, key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        while self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ed25519Verifier.verify(key, message, signature)
    }
}

/// `OwnerOnly`, but `may_stop` waits first for as long as the flag is set.
struct Waits(AtomicBool);

impl StopPermission for Waits {
    fn may_stop(&self, person: &str, owner: &str) -> bool {
        while self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        OwnerOnly.may_stop(person, owner)
    }
    fn may_reopen(&self, person: &str, owner: &str) -> bool {
        OwnerOnly.may_reopen(person, owner)
    }
}

/// Pull a stop while the judge blocks, and check that it ends, holds, and survives a restart.
async fn blocked_stop(
    block: &AtomicBool,
    verifier: Arc<dyn DeviceVerifier>,
    perm: Arc<dyn StopPermission>,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("guard.db"), encrypted())
            .await
            .unwrap(),
    );
    let clock = Arc::new(TestClock::default());
    let (engine, enrolment) = GuardEngine::open_with(
        config_in(dir.path()),
        Faulty::over(store.clone()),
        verifier,
        perm,
        clock.clone(),
    )
    .unwrap();
    let owner = Device::new(OWNER, 1);
    enrolment.enrol_owner(&owner.public()).await.unwrap();
    let identity = engine.engine_identity().await.unwrap();
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(engine.state().await.mode, GuardMode::Open);

    block.store(true, Ordering::SeqCst);
    let proof = owner.proof(&identity, Method::Stop, &[]);
    let pulled = tokio::time::timeout(Duration::from_secs(5), engine.stop(&proof)).await;
    block.store(false, Ordering::SeqCst);
    assert!(
        pulled.is_ok(),
        "the stop ended although the judge never answered"
    );
    assert_eq!(engine.state().await.mode, GuardMode::Stopped, "in memory");
    assert!(config_in(dir.path()).stop_marker.exists(), "and on disk");
    assert!(!engine.stop_unwritten(), "the marker is down");
    drop((engine, enrolment));

    let (engine, _enrolment) = loop {
        let Ok(real) = fs_messenger_guard::TursoGuardRepository::open(store.clone()) else {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        };
        break GuardEngine::open_with(
            config_in(dir.path()),
            Faulty::wrap(real),
            Arc::new(Ed25519Verifier),
            Arc::new(OwnerOnly),
            clock,
        )
        .unwrap();
    };
    assert_eq!(
        engine.state().await.mode,
        GuardMode::Stopped,
        "after a restart"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verifier_that_never_answers_does_not_lose_the_stop() {
    let blocks = Arc::new(Blocks(AtomicBool::new(false)));
    blocked_stop(&blocks.0, blocks.clone(), Arc::new(OwnerOnly)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_permission_that_never_answers_does_not_lose_the_stop() {
    let waits = Arc::new(Waits(AtomicBool::new(false)));
    blocked_stop(&waits.0, Arc::new(Ed25519Verifier), waits.clone()).await;
}
