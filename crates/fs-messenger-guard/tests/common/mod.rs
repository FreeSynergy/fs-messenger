//! Shared test helpers: real encrypted `TursoStore`s in temp files, a real messenger, real signatures.
#![allow(dead_code, unused_imports)] // each test file uses a different subset

pub mod destination;
pub mod device;
pub mod events;
pub mod ideas;
pub mod program;
pub mod rigs;

pub use device::{Device, TestClock};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use ed25519_compact::{KeyPair, Seed};
use fs_messenger_core::{
    Account, AccountSetup, Ingest, Ingested, Kind, Messenger, MessengerConfig, MessengerRepo,
    RawMessage, ToolCall, ToolCallDecision, ToolCallRisk,
};
use fs_messenger_guard::{
    AiGrant, Arguments, Caller, Clock, DeviceVerifier, Ed25519Verifier, Gate, Guard, GuardConfig,
    GuardEngine, GuardProof, LocalEnrolment, Method, OwnerOnly, Question, STATE_ID,
    TursoGuardRepository, Verdict, decide_arguments, signed_bytes,
};
use fs_objects::traits::UnloggedObjects;
use fs_objects::{Cipher, EncryptionKey, ObjectId, OpenOptions, StaticKeyProvider, TursoStore};
use serde_json::{Value, json};
use tempfile::TempDir;
pub const OWNER: &str = "https://local.invalid/users/owner";
pub const FRIEND: &str = "https://local.invalid/users/friend";
pub const AI: &str = "https://local.invalid/ai/helper";
pub const OTHER_AI: &str = "https://local.invalid/ai/other";

pub fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

/// One program, on real stores in files that survive a restart.
pub struct World {
    pub dir: TempDir,
    pub store: Arc<TursoStore>,
    pub messages: Arc<TursoStore>,
    pub repo: MessengerRepo,
    pub engine: Arc<GuardEngine>,
    pub enrolment: LocalEnrolment,
    pub clock: Arc<TestClock>,
    pub owner: Device,
    pub identity: Vec<u8>,
}

async fn turso(path: PathBuf) -> Arc<TursoStore> {
    Arc::new(TursoStore::open_with(path, encrypted()).await.unwrap())
}

pub fn engine_on(
    store: &Arc<TursoStore>,
    clock: &Arc<TestClock>,
    marker: &Path,
) -> (Arc<GuardEngine>, LocalEnrolment) {
    let repo = Arc::new(TursoGuardRepository::open(store.clone()).unwrap());
    GuardEngine::open_with(
        GuardConfig::new(OWNER, marker),
        repo,
        Arc::new(Ed25519Verifier) as Arc<dyn DeviceVerifier>,
        Arc::new(OwnerOnly),
        clock.clone(),
    )
    .unwrap()
}

impl World {
    /// Fresh: no key yet, nothing opened.
    pub async fn fresh() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = turso(dir.path().join("guard.db")).await;
        let messages = turso(dir.path().join("messages.db")).await;
        let repo = MessengerRepo::open_turso(messages.clone(), MessengerConfig::new(OWNER))
            .await
            .unwrap();
        let clock = Arc::new(TestClock::default());
        let (engine, enrolment) = engine_on(&store, &clock, &dir.path().join("guard.stopped"));
        engine.attach(&repo).unwrap();
        Self {
            dir,
            store,
            messages,
            repo,
            engine,
            enrolment,
            clock,
            owner: Device::new(OWNER, 1),
            identity: vec![],
        }
    }

    /// The owner's key is in, and the program is open.
    pub async fn open() -> Self {
        let mut w = Self::fresh().await;
        w.enrolment.enrol_owner(&w.owner.public()).await.unwrap();
        w.identity = w.engine.engine_identity().await.unwrap();
        let proof = w.owner.proof(&w.identity, Method::Reopen, &[]);
        w.engine.reopen(&proof).await.unwrap();
        w
    }

    /// Open, with `AI` invited and nothing released.
    pub async fn with_ai() -> Self {
        let w = Self::open().await;
        w.invite(AI).await;
        w
    }

    /// As after a restart: every handle dropped, and the files opened again. The device stays.
    pub async fn restart(self) -> Self {
        let Self {
            dir,
            store,
            messages,
            repo,
            engine,
            enrolment,
            clock,
            owner,
            identity,
        } = self;
        drop((store, messages, repo, engine, enrolment));
        let store = turso(dir.path().join("guard.db")).await;
        let messages = turso(dir.path().join("messages.db")).await;
        let repo = MessengerRepo::open_turso(messages.clone(), MessengerConfig::new(OWNER))
            .await
            .unwrap();
        let (engine, enrolment) = engine_on(&store, &clock, &dir.path().join("guard.stopped"));
        engine.attach(&repo).unwrap();
        Self {
            dir,
            store,
            messages,
            repo,
            engine,
            enrolment,
            clock,
            owner,
            identity,
        }
    }

    /// Another conversation, with a message in it.
    pub async fn repo_conversation(&self, id: &str) -> ObjectId {
        let account = self.account(&format!("{id}@example.org"), "mail").await;
        self.deliver(account.id, id, "x@y").await.unwrap();
        let page = self.repo.conversations(None, false, None).await.unwrap();
        page.items[0].id
    }

    /// Past the pause between retries.
    pub fn pause(&self) -> &Self {
        self.clock.advance(Duration::seconds(3));
        self
    }

    pub async fn invite(&self, profile: &str) {
        let proof = self
            .owner
            .proof(&self.identity, Method::Invite, &[profile.as_bytes()]);
        self.engine.invite(profile, &proof).await.unwrap();
    }

    pub fn release_proof(&self, profile: &str, risk: &str) -> GuardProof {
        self.owner.proof(
            &self.identity,
            Method::Release,
            &[profile.as_bytes(), risk.as_bytes(), PROGRAM.as_bytes()],
        )
    }

    pub async fn release(&self, profile: &str, risk: ToolCallRisk, name: &str) {
        let grant = AiGrant {
            profile: profile.to_string(),
            risk,
            program: PROGRAM.into(),
        };
        self.engine
            .release(&grant, &self.release_proof(profile, name))
            .await
            .unwrap();
    }

    pub fn stop_proof(&self) -> GuardProof {
        self.owner.proof(&self.identity, Method::Stop, &[])
    }

    pub fn reopen_proof(&self) -> GuardProof {
        self.owner.proof(&self.identity, Method::Reopen, &[])
    }

    /// `check` that has to come back as a question. Moves the clock past the retry pause first.
    pub async fn ask(&self, capability: &str, args: Value) -> Question {
        self.clock.advance(Duration::seconds(3));
        let mut verdict = self.check(capability, args.clone()).await;
        if verdict == Verdict::Refuse(fs_messenger_guard::Refusal::NotReleased) {
            // A question needs a release in the tool's own program (ADR 0054); give the least one.
            self.unlock_program_of(capability).await;
            verdict = self.check(capability, args).await;
        }
        match verdict {
            Verdict::Ask(q) => *q,
            other => panic!("expected a question, got {other:?}"),
        }
    }

    pub async fn check(&self, capability: &str, args: Value) -> Verdict {
        let arguments = Arguments::from_value(&args).unwrap();
        self.engine
            .check(&Caller::new(AI), capability, &arguments)
            .await
    }

    /// The record as stored, byte for byte.
    pub async fn record(&self) -> Option<Value> {
        let id: ObjectId = STATE_ID.parse().unwrap();
        self.store
            .read_unlogged(id)
            .await
            .unwrap()
            .map(|o| o.content)
    }

    /// Overwrite the stored record, as someone with the store in hand could.
    pub async fn tamper(&self, change: impl FnOnce(&mut Value)) {
        let id: ObjectId = STATE_ID.parse().unwrap();
        let mut object = self.store.read_unlogged(id).await.unwrap().unwrap();
        change(&mut object.content);
        self.store.put_unlogged(object).await.unwrap();
    }

    pub async fn account(&self, address: &str, kind: &str) -> Account {
        self.repo
            .add_account(AccountSetup {
                address: address.into(),
                kind: kind.into(),
                secret: String::new(),
            })
            .await
            .unwrap()
    }

    pub async fn deliver(
        &self,
        account: ObjectId,
        id: &str,
        sender: &str,
    ) -> fs_messenger_core::Result<Ingested> {
        let mut raw = RawMessage::new(
            fs_messenger_core::ExternalId::new("uid", id),
            fs_messenger_core::ExternalId::new("thread", "t"),
            Kind::Chat,
            sender,
            "hello",
        );
        raw.encryption = fs_messenger_core::Encryption::Mls;
        self.repo.ingest(account, raw).await
    }

    pub async fn shown(&self, q: &Question) -> ToolCall {
        let m = self.repo.message(q.conversation, q.message).await.unwrap();
        m.tool_calls
            .iter()
            .find(|c| c.id == q.call.id)
            .unwrap()
            .clone()
    }

    pub fn doc_mode(record: &Value) -> &str {
        record["mode"].as_str().unwrap()
    }
}

/// The program a plain `release` of the helpers names.
pub const PROGRAM: &str = "message";

pub fn args(path: &str) -> Value {
    json!({ "path": path })
}
