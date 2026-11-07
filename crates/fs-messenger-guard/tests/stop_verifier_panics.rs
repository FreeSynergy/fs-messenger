//! Blocker: a verifier that panics while a stop is judged cannot lose the stop. It stays in force
//! now, is written to the marker, and is there after a restart even if the store still says "open".

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::rigs::{Faulty, config_in};
use common::{Device, OWNER, TestClock, encrypted};
use fs_messenger_guard::{
    DeviceVerifier, Ed25519Verifier, Guard, GuardEngine, GuardMode, Method, OwnerOnly,
};
use fs_store::TursoStore;

/// Ed25519, until it is told to panic.
struct Breaks(AtomicBool);

impl DeviceVerifier for Breaks {
    fn key_type(&self) -> &'static str {
        Ed25519Verifier.key_type()
    }
    fn accepts_key(&self, key: &[u8]) -> bool {
        Ed25519Verifier.accepts_key(key)
    }
    fn verify(&self, key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        assert!(
            !self.0.load(Ordering::SeqCst),
            "a foreign verifier falls over"
        );
        Ed25519Verifier.verify(key, message, signature)
    }
}

#[tokio::test]
async fn a_stop_whose_judge_panics_is_still_a_stop_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("guard.db"), encrypted())
            .await
            .unwrap(),
    );
    let clock = Arc::new(TestClock::default());
    let breaks = Arc::new(Breaks(AtomicBool::new(false)));
    let (engine, enrolment) = GuardEngine::open_with(
        config_in(dir.path()),
        Faulty::over(store.clone()),
        breaks.clone(),
        Arc::new(OwnerOnly),
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

    breaks.0.store(true, Ordering::SeqCst);
    let pulled = engine
        .stop(&owner.proof(&identity, Method::Stop, &[]))
        .await;
    breaks.0.store(false, Ordering::SeqCst);
    assert!(pulled.is_ok() || pulled.is_err(), "either way it ends");
    assert_eq!(engine.state().await.mode, GuardMode::Stopped, "in memory");
    assert!(
        config_in(dir.path()).stop_marker.exists(),
        "and on disk, beside the store"
    );
    // The store may never have seen it: after a restart only the marker is left.
    drop((engine, enrolment));

    let (engine, _enrolment) = loop {
        let repo = match fs_messenger_guard::TursoGuardRepository::open(store.clone()) {
            Ok(real) => Faulty::wrap(real),
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
        };
        break GuardEngine::open_with(
            config_in(dir.path()),
            repo,
            Arc::new(Ed25519Verifier),
            Arc::new(OwnerOnly),
            clock.clone(),
        )
        .unwrap();
    };
    assert_eq!(engine.state().await.mode, GuardMode::Stopped);
    // A written reopen still works: nothing stays held for good.
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(engine.state().await.mode, GuardMode::Open);
}
