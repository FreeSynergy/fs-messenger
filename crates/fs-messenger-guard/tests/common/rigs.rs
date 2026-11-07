//! A real repository whose calls can fail, hang or panic, and a program built on it.
#![allow(dead_code)]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use fs_messenger_core::{MessengerConfig, MessengerRepo, ToolCallRisk};
use fs_messenger_guard::{
    AiGrant, Arguments, Caller, Ed25519Verifier, Gate, Guard, GuardConfig, GuardEngine,
    GuardLogEntry, GuardRepository, LocalEnrolment, Method, OwnerOnly, TursoGuardRepository,
    Verdict,
};
use fs_store::TursoStore;
use serde_json::Value;
use tempfile::TempDir;
use tokio::sync::Notify;

use super::{AI, Device, OWNER, TestClock, encrypted};

/// The real repository, with faults that a test turns on.
pub struct Faulty {
    pub real: TursoGuardRepository,
    pub fail_writes: AtomicBool,
    pub hang_loads: AtomicBool,
    pub hang_saves: AtomicBool,
    pub panic_saves: AtomicBool,
    pub panic_appends: AtomicBool,
    /// The next load tells `entered` and waits for `go`.
    pub hold_load: AtomicBool,
    /// The next save does the same.
    pub hold_save: AtomicBool,
    /// The next log entry does the same.
    pub hold_append: AtomicBool,
    pub fail_reads: AtomicBool,
    pub entered: Notify,
    pub go: Notify,
}

impl Faulty {
    pub fn over(store: Arc<TursoStore>) -> Arc<Self> {
        Self::wrap(TursoGuardRepository::open(store).unwrap())
    }

    pub fn wrap(real: TursoGuardRepository) -> Arc<Self> {
        Arc::new(Self {
            real,
            fail_writes: AtomicBool::new(false),
            hang_loads: AtomicBool::new(false),
            hang_saves: AtomicBool::new(false),
            panic_saves: AtomicBool::new(false),
            panic_appends: AtomicBool::new(false),
            hold_load: AtomicBool::new(false),
            hold_save: AtomicBool::new(false),
            hold_append: AtomicBool::new(false),
            fail_reads: AtomicBool::new(false),
            entered: Notify::new(),
            go: Notify::new(),
        })
    }
}

#[async_trait]
impl GuardRepository for Faulty {
    async fn load(&self) -> Result<Option<Value>, String> {
        if self.hang_loads.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err("the store does not answer".into());
        }
        let loaded = self.real.load().await;
        if self.hold_load.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.go.notified().await;
        }
        loaded
    }
    async fn save(&self, record: &Value) -> Result<(), String> {
        if self.hang_saves.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        assert!(
            !self.panic_saves.load(Ordering::SeqCst),
            "a save falls over"
        );
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err("disk full".into());
        }
        if self.hold_save.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.go.notified().await;
        }
        self.real.save(record).await
    }
    async fn append(&self, entry: &GuardLogEntry) -> Result<(), String> {
        assert!(
            !self.panic_appends.load(Ordering::SeqCst),
            "a log falls over"
        );
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err("disk full".into());
        }
        if self.hold_append.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.go.notified().await;
        }
        self.real.append(entry).await
    }
    async fn entries(&self) -> Result<Vec<GuardLogEntry>, String> {
        self.real.entries().await
    }
}

/// Quick to give up and quick to try again, so that the tests do not wait.
pub fn config_in(dir: &std::path::Path) -> GuardConfig {
    let mut config = GuardConfig::new(OWNER, dir.join("guard.stopped"));
    config.store_wait = Duration::from_millis(300);
    config.sender_wait = Duration::from_millis(100);
    config.stop_retry = Duration::from_millis(50);
    config.turn_wait = Duration::from_millis(500);
    config
}

/// Until the condition holds, within reason.
pub async fn eventually(what: &str, mut holds: impl AsyncFnMut() -> bool) {
    for _ in 0..200 {
        if holds().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never: {what}");
}

/// A program on a faulty guard store and a real messenger, open, `AI` invited with `readsOnly`.
pub struct Rig {
    pub dir: TempDir,
    pub store: Arc<TursoStore>,
    pub faulty: Arc<Faulty>,
    pub repo: MessengerRepo,
    pub engine: Arc<GuardEngine>,
    pub enrolment: LocalEnrolment,
    pub clock: Arc<TestClock>,
    pub owner: Device,
    pub identity: Vec<u8>,
}

async fn turso(path: std::path::PathBuf) -> Arc<TursoStore> {
    Arc::new(TursoStore::open_with(path, encrypted()).await.unwrap())
}

pub fn engine_over(
    faulty: Arc<Faulty>,
    dir: &std::path::Path,
    clock: Arc<TestClock>,
) -> (Arc<GuardEngine>, LocalEnrolment) {
    GuardEngine::open_with(
        config_in(dir),
        faulty,
        Arc::new(Ed25519Verifier),
        Arc::new(OwnerOnly),
        clock,
    )
    .unwrap()
}

impl Rig {
    pub async fn open() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = turso(dir.path().join("guard.db")).await;
        let messages = turso(dir.path().join("messages.db")).await;
        let repo = MessengerRepo::open_turso(messages, MessengerConfig::new(OWNER))
            .await
            .unwrap();
        let faulty = Faulty::over(store.clone());
        let clock = Arc::new(TestClock::default());
        let (engine, enrolment) = engine_over(faulty.clone(), dir.path(), clock.clone());
        engine.attach(&repo).unwrap();
        let owner = Device::new(OWNER, 1);
        enrolment.enrol_owner(&owner.public()).await.unwrap();
        let identity = engine.engine_identity().await.unwrap();
        engine
            .reopen(&owner.proof(&identity, Method::Reopen, &[]))
            .await
            .unwrap();
        let invite = owner.proof(&identity, Method::Invite, &[AI.as_bytes()]);
        engine.invite(AI, &invite).await.unwrap();
        let release = owner.proof(&identity, Method::Release, &[AI.as_bytes(), b"readsOnly"]);
        let grant = AiGrant {
            profile: AI.into(),
            risk: ToolCallRisk::ReadsOnly,
        };
        engine.release(&grant, &release).await.unwrap();
        Self {
            dir,
            store,
            faulty,
            repo,
            engine,
            enrolment,
            clock,
            owner,
            identity,
        }
    }

    pub fn proof(&self, method: Method) -> fs_messenger_guard::GuardProof {
        self.owner.proof(&self.identity, method, &[])
    }

    pub async fn read_check(&self) -> Verdict {
        let input = Arguments::from_text(r#"{"path":"/"}"#).unwrap();
        self.engine
            .check(&Caller::new(AI), "message.open", &input)
            .await
    }
}
