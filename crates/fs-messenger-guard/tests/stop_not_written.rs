//! A stop that cannot be written still stops, and says so. Real store underneath; only the failure of a
//! write or read is injected.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use common::{AI, Device, OWNER, TestClock, World, args, encrypted};
use fs_messenger_core::{MessageAction, Messenger, ToolCallRisk};
use fs_messenger_guard::{
    Arguments, Caller, Ed25519Verifier, Gate, Guard, GuardConfig, GuardEngine, GuardError,
    GuardLogEntry, GuardMode, GuardRepository, Method, OwnerOnly, Refusal, STATE_ID,
    TursoGuardRepository, Verdict,
};
use fs_objects::traits::UnloggedObjects;
use fs_objects::{ObjectId, TursoStore};
use serde_json::{Value, json};
use tokio::sync::Notify;
/// The real repository, whose writes can fail and whose reads can stop answering.
struct Faulty {
    real: TursoGuardRepository,
    fail_writes: AtomicBool,
    fail_reads: AtomicBool,
    /// The next save tells `entered` and waits for `go` first.
    hold_save: AtomicBool,
    entered: Notify,
    go: Notify,
}

#[async_trait]
impl GuardRepository for Faulty {
    async fn load(&self) -> Result<Option<Value>, String> {
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err("the store does not answer".into());
        }
        self.real.load().await
    }
    async fn save(&self, record: &Value) -> Result<(), String> {
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
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err("disk full".into());
        }
        self.real.append(entry).await
    }
    async fn entries(&self) -> Result<Vec<GuardLogEntry>, String> {
        self.real.entries().await
    }
}

struct Rig {
    dir: tempfile::TempDir,
    store: Arc<TursoStore>,
    faulty: Arc<Faulty>,
    engine: Arc<GuardEngine>,
    owner: Device,
    identity: Vec<u8>,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("g.db"), encrypted())
            .await
            .unwrap(),
    );
    let faulty = Arc::new(Faulty {
        real: TursoGuardRepository::open(store.clone()).unwrap(),
        fail_writes: AtomicBool::new(false),
        fail_reads: AtomicBool::new(false),
        hold_save: AtomicBool::new(false),
        entered: Notify::new(),
        go: Notify::new(),
    });
    let (engine, enrolment) = GuardEngine::open_with(
        GuardConfig::new(OWNER),
        faulty.clone(),
        Arc::new(Ed25519Verifier),
        Arc::new(OwnerOnly),
        Arc::new(TestClock::default()),
    );
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
    let grant = fs_messenger_guard::AiGrant {
        profile: AI.into(),
        risk: ToolCallRisk::ReadsOnly,
    };
    engine.release(&grant, &release).await.unwrap();
    Rig {
        dir,
        store,
        faulty,
        engine,
        owner,
        identity,
    }
}

async fn verdict(engine: &GuardEngine) -> Verdict {
    let input = Arguments::from_value(&args("/")).unwrap();
    engine.check(&Caller::new(AI), "message.open", &input).await
}

#[tokio::test]
async fn a_stop_that_cannot_be_written_still_stops_and_says_so() {
    let r = rig().await;
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

    // Once writing works again, the next `check` puts it on disk, and a restart keeps it.
    r.faulty.fail_writes.store(false, Ordering::SeqCst);
    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    let record = r.faulty.real.load().await.unwrap().unwrap();
    assert_eq!(World::doc_mode(&record), "stopped");
    assert_eq!(record["grants"], json!([]));
    drop(r.dir);
    drop(r.store);
}

#[tokio::test]
async fn a_reopen_that_cannot_be_written_stays_shut() {
    let r = rig().await;
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
    let r = rig().await;
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
    assert!(
        matches!(verdict(&r.engine).await, Verdict::Refuse(_)),
        "no messenger to ask"
    );
}

#[tokio::test]
async fn when_the_store_stops_answering_everything_is_refused_and_comes_back_only_as_it_was() {
    let r = rig().await;
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
    let r = rig().await;
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
    reopening.await.unwrap().unwrap();
    stopping.await.unwrap().unwrap();

    assert_eq!(verdict(&r.engine).await, Verdict::Refuse(Refusal::Stopped));
    let record = r.faulty.real.load().await.unwrap().unwrap();
    assert_eq!(
        World::doc_mode(&record),
        "stopped",
        "and it stays so after a restart"
    );
}
