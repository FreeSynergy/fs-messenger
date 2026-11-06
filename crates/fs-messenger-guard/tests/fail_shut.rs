//! When in doubt it stays shut: an unreadable, missing or corrupt state reads as stopped, and a stop
//! that cannot be written still stops. Real store underneath; only the failure of a write is injected.

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
use fs_store::traits::UnloggedObjects;
use fs_store::{Envelope, ObjectId, StoredObject, TursoStore};
use serde_json::{Value, json};

async fn stopped_after(w: &World) {
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert!(w.engine.state().await.grants.is_empty());
    assert!(w.engine.state().await.invited.is_empty());
}

#[tokio::test]
async fn each_kind_of_corrupt_record_reads_as_stopped_also_after_a_restart() {
    let corrupt: Vec<(&str, Value)> = vec![
        ("not an object", json!("oops")),
        ("empty", json!({})),
        ("null", Value::Null),
        ("a newer version", json!({ "version": 99 })),
    ];
    for (name, value) in corrupt {
        let w = World::with_ai().await;
        w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
        w.tamper(|doc| *doc = value.clone()).await;
        assert!(w.engine.damage().await.is_some(), "{name}");
        stopped_after(&w).await;
        let w = w.restart().await;
        stopped_after(&w).await;
    }
}

#[tokio::test]
async fn a_record_that_lost_one_field_reads_as_stopped() {
    for field in [
        "mode",
        "engine",
        "devices",
        "invited",
        "grants",
        "calls",
        "usedProofs",
        "version",
    ] {
        let w = World::with_ai().await;
        w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
        w.tamper(|doc| {
            doc.as_object_mut().unwrap().remove(field);
        })
        .await;
        stopped_after(&w).await;
    }
}

#[tokio::test]
async fn an_unrecognised_mode_reads_as_stopped() {
    let w = World::with_ai().await;
    w.tamper(|doc| doc["mode"] = json!("wideOpen")).await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
}

#[tokio::test]
async fn a_deleted_state_reads_as_stopped_like_a_fresh_program() {
    let w = World::with_ai().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    w.store.delete_unlogged(id).await.unwrap();
    stopped_after(&w).await;
    assert!(w.engine.damage().await.is_none(), "it is simply not there");
    let w = w.restart().await;
    stopped_after(&w).await;
}

#[tokio::test]
async fn an_object_of_another_type_under_the_id_reads_as_stopped() {
    let w = World::with_ai().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    let mut good = w.store.read_unlogged(id).await.unwrap().unwrap();
    let content = good.content.clone();
    good.envelope.object_type = "Note".into();
    good.content = content;
    w.store.put_unlogged(good).await.unwrap();
    stopped_after(&w).await;
}

#[tokio::test]
async fn while_damaged_nothing_can_be_signed_and_the_stop_says_so() {
    let w = World::with_ai().await;
    let (stop, reopen) = (w.stop_proof(), w.reopen_proof());
    w.tamper(|doc| *doc = json!("oops")).await;
    let report = w.engine.stop(&stop).await.unwrap();
    assert!(report.not_persisted.is_some(), "said, not hidden");
    assert!(matches!(
        w.engine.reopen(&reopen).await,
        Err(GuardError::Refused(Refusal::Damaged))
    ));
    assert!(matches!(
        w.enrolment.enrol_owner(&w.owner.public()).await,
        Err(GuardError::Refused(Refusal::Damaged))
    ));
}

#[tokio::test]
async fn recovery_is_local_starts_stopped_and_kills_every_older_proof() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let old_reopen = w.reopen_proof();
    let old_identity = w.identity.clone();
    // Not on a healthy state.
    assert!(matches!(
        w.enrolment.recover_damaged(&w.owner.public()).await,
        Err(GuardError::Refused(Refusal::NotEnrollable(_)))
    ));
    w.tamper(|doc| *doc = json!("oops")).await;
    w.enrolment
        .recover_damaged(&w.owner.public())
        .await
        .unwrap();
    stopped_after(&w).await;
    let identity = w.engine.engine_identity().await.unwrap();
    assert_ne!(identity, old_identity);
    assert!(matches!(
        w.engine.reopen(&old_reopen).await,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    w.engine
        .reopen(&w.owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    assert!(w.engine.state().await.grants.is_empty());
}

/// The real repository, whose writes can fail and whose reads can stop answering.
struct Faulty {
    real: TursoGuardRepository,
    fail_writes: AtomicBool,
    fail_reads: AtomicBool,
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
    use fs_store::Store;
    w.store.put(current, "someone").await.unwrap();
    assert_eq!(
        w.check("message.delete", args("/m")).await,
        Verdict::Refuse(Refusal::NotRecorded)
    );
}

#[tokio::test]
async fn the_repository_refuses_a_store_that_is_not_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Arc::new(TursoStore::open(dir.path().join("p.db")).await.unwrap());
    assert!(TursoGuardRepository::open(plain).is_err());
    let memory = Arc::new(TursoStore::open_in_memory().await.unwrap());
    assert!(TursoGuardRepository::open(memory).is_err());
}

#[tokio::test]
async fn the_state_object_is_marked_sensitive() {
    let w = World::open().await;
    let id: ObjectId = STATE_ID.parse().unwrap();
    let object: StoredObject = w.store.read_unlogged(id).await.unwrap().unwrap();
    assert!(!object.envelope.sensitive.is_empty());
    let _ = Envelope::new("x", "y");
}
