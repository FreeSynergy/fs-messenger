//! A real messenger and a real guard on encrypted `TursoStore`s in temp files, a real device signature,
//! and the real provider. What stands in: the wire (`wire.rs`), and `PastTheGate`, which is labelled.
#![allow(dead_code, unused_imports)]

pub mod events;
pub mod wire;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ed25519_compact::{KeyPair, Seed};
use fs_interfaces::provider::Capabilities;
use fs_messenger_ai::{Endpoint, Host, ModelConfig, ModelProvider, ProviderLink};
use fs_messenger_core::{
    AccountSetup, DeliveryState, EncryptionPlan, Error, Event, Message, Messenger, MessengerConfig,
    MessengerRepo, Outgoing, Provider, SendReceipt, TurnScope,
};
use fs_messenger_guard::{
    Guard, GuardConfig, GuardEngine, GuardProof, Method, TursoGuardRepository, signed_bytes,
};
use fs_objects::{Cipher, EncryptionKey, ObjectId, OpenOptions, StaticKeyProvider, TursoStore};
use tempfile::TempDir;
use tokio::sync::broadcast;

pub const OWNER: &str = "https://local.invalid/users/owner";
pub const AI: &str = "https://local.invalid/ai/helper";
pub const OTHER_AI: &str = "https://local.invalid/ai/other";
/// The model's name is configuration; only tests have one.
pub const MODEL: &str = "scripted-model";
pub const KEY: &str = "sk-needle-4711-do-not-print";

fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

async fn turso(path: std::path::PathBuf) -> Arc<TursoStore> {
    Arc::new(TursoStore::open_with(path, encrypted()).await.unwrap())
}

/// **Stands in for a decision the owner has not made.** The core refuses every honest plan of a model, so
/// to reach `Provider::send` at all the plan has to lie, here and nowhere in the crate. Everything behind
/// it is the real provider, which reports the truth in its receipt whatever this says.
pub struct PastTheGate(pub Arc<ModelProvider>);

#[async_trait]
impl Provider for PastTheGate {
    fn kind(&self) -> fs_messenger_core::Result<String> {
        self.0.kind()
    }
    fn capabilities(&self) -> Capabilities {
        self.0.capabilities()
    }
    async fn encryption_plan(&self, _: &[String]) -> fs_messenger_core::Result<EncryptionPlan> {
        Ok(EncryptionPlan {
            encryption: fs_messenger_core::Encryption::Mls,
            missing_keys: vec![],
        })
    }
    async fn send(&self, message: &Outgoing) -> fs_messenger_core::Result<SendReceipt> {
        self.0.send(message).await
    }
    async fn stop_turns(&self, scope: &TurnScope) -> fs_messenger_core::Result<()> {
        self.0.stop_turns(scope).await
    }
}

/// One program.
pub struct World {
    pub dir: TempDir,
    pub messages: Arc<TursoStore>,
    pub repo: Arc<MessengerRepo>,
    pub engine: Arc<GuardEngine>,
    pub pair: KeyPair,
    pub identity: Vec<u8>,
    pub account: ObjectId,
    nonce: std::sync::atomic::AtomicU64,
}

impl World {
    /// Open, with an `agent` account, and nobody invited.
    pub async fn uninvited() -> Self {
        Self::with_config(MessengerConfig::new(OWNER)).await
    }

    /// As `uninvited`, with this messenger configuration.
    pub async fn with_config(config: MessengerConfig) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let messages = turso(dir.path().join("messages.db")).await;
        let guard_store = turso(dir.path().join("guard.db")).await;
        let repo = MessengerRepo::open_turso(messages.clone(), config)
            .await
            .unwrap();
        let guard_repo = Arc::new(TursoGuardRepository::open(guard_store).unwrap());
        let (engine, enrolment) = GuardEngine::open(GuardConfig::new(OWNER), guard_repo);
        engine.attach(&repo).unwrap();
        let pair = KeyPair::from_seed(Seed::new([1; 32]));
        enrolment.enrol_owner(pair.pk.as_ref()).await.unwrap();
        let identity = engine.engine_identity().await.unwrap();
        let account = repo
            .add_account(AccountSetup {
                address: "models@local.invalid".into(),
                kind: "agent".into(),
                secret: String::new(),
            })
            .await
            .unwrap()
            .id;
        let world = Self {
            dir,
            messages,
            repo: Arc::new(repo),
            engine,
            pair,
            identity,
            account,
            nonce: std::sync::atomic::AtomicU64::new(1),
        };
        world
            .engine
            .reopen(&world.proof(Method::Reopen, &[]))
            .await
            .unwrap();
        world
    }

    /// Open, with `AI` invited and nothing released.
    pub async fn invited() -> Self {
        let world = Self::uninvited().await;
        world.invite(AI).await;
        world
    }

    pub fn proof(&self, method: Method, arguments: &[&[u8]]) -> GuardProof {
        let n = self.nonce.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut nonce = vec![9; 8];
        nonce.extend_from_slice(&n.to_be_bytes());
        let bytes = signed_bytes(&self.identity, method, OWNER, arguments, &nonce).unwrap();
        GuardProof {
            signer: OWNER.into(),
            nonce,
            proof: self.pair.sk.sign(&bytes, None).to_vec(),
        }
    }

    pub async fn invite(&self, profile: &str) {
        let proof = self.proof(Method::Invite, &[profile.as_bytes()]);
        self.engine.invite(profile, &proof).await.unwrap();
    }

    pub async fn emergency_stop(&self) {
        let proof = self.proof(Method::Stop, &[]);
        self.engine.stop(&proof).await.unwrap();
    }

    pub fn link(&self) -> ProviderLink {
        let repo: std::sync::Weak<MessengerRepo> = Arc::downgrade(&self.repo);
        let host: std::sync::Weak<dyn Host> = repo;
        ProviderLink {
            account: self.account,
            host,
            gate: self.engine.clone(),
        }
    }

    /// A configuration for the origin, with short limits for what is not under test.
    pub fn config(&self, origin: &str) -> ModelConfig {
        let endpoint = Endpoint::parse(origin).unwrap();
        ModelConfig::new(AI, MODEL, endpoint).unwrap()
    }

    pub async fn llama(&self, config: ModelConfig) -> Arc<ModelProvider> {
        let provider = ModelProvider::llama_cpp(config, self.link()).await.unwrap();
        Arc::new(provider)
    }

    pub async fn anthropic(&self, mut config: ModelConfig) -> Arc<ModelProvider> {
        config.api_key = Some(fs_messenger_ai::ApiKey::new(KEY));
        let provider = ModelProvider::anthropic(config, self.link()).await.unwrap();
        Arc::new(provider)
    }

    /// The provider as the owner's decision would have it: past the gate. See [`PastTheGate`].
    pub fn past_the_gate(&self, provider: &Arc<ModelProvider>) {
        let wrapped: Arc<dyn Provider> = Arc::new(PastTheGate(provider.clone()));
        self.repo.register_provider(self.account, wrapped);
    }

    /// A person writes `text` to `to` and presses send.
    pub async fn person_sends(&self, to: &str, text: &str) -> fs_messenger_core::Result<ObjectId> {
        let mut draft = self.repo.compose(None).await?;
        draft.set_account(self.account).await?;
        draft
            .set_recipients(vec![to.to_string()], vec![], vec![])
            .await?;
        draft
            .set_text(text.to_string(), "text/plain".to_string())
            .await?;
        draft.send(None).await
    }

    /// A reply in the conversation of `to_message`.
    pub async fn person_replies(
        &self,
        to_message: ObjectId,
        text: &str,
    ) -> fs_messenger_core::Result<ObjectId> {
        let mut draft = self.repo.compose(Some(to_message)).await?;
        draft
            .set_text(text.to_string(), "text/plain".to_string())
            .await?;
        draft.send(None).await
    }

    /// One whole turn, past the gate: the send answers `EncryptionDowngraded`, which is the honest
    /// receipt meeting the lie of [`PastTheGate`], and the turn runs.
    pub async fn turn(&self, provider: &Arc<ModelProvider>, text: &str) -> Finished {
        self.past_the_gate(provider);
        let mut events = provider.live_events();
        let sent = self.person_sends(AI, text).await;
        assert!(
            matches!(sent, Err(Error::EncryptionDowngraded { .. })),
            "{sent:?}"
        );
        self.finished(provider, &mut events).await
    }

    /// Waits for the state event of a turn, and collects what came.
    pub async fn finished(
        &self,
        provider: &Arc<ModelProvider>,
        events: &mut broadcast::Receiver<Event>,
    ) -> Finished {
        let events = until_state(events).await;
        let Some(Event::State(state)) = events.last() else {
            panic!("no state event");
        };
        let live = provider.live_turn(state.message).expect("the turn is kept");
        let stored = match live.stored {
            Some(id) => Some(self.repo.message(live.conversation, id).await.unwrap()),
            None => None,
        };
        Finished {
            events,
            live,
            stored,
        }
    }
}

/// What a turn left.
pub struct Finished {
    pub events: Vec<Event>,
    pub live: fs_messenger_ai::LiveTurn,
    pub stored: Option<Message>,
}

impl Finished {
    pub fn answer(&self) -> &Message {
        self.stored.as_ref().expect("a stored answer")
    }
    pub fn failure(&self) -> fs_messenger_ai::TurnFailure {
        self.live.failure.clone().expect("a failed turn")
    }
    pub fn sent(&self) -> bool {
        self.live.state == DeliveryState::Sent && self.live.failure.is_none()
    }
}

pub async fn until_state(events: &mut broadcast::Receiver<Event>) -> Vec<Event> {
    let mut all = vec![];
    loop {
        let next = tokio::time::timeout(Duration::from_secs(30), events.recv());
        let event = next.await.expect("a turn ends").expect("the feed stays");
        let last = matches!(event, Event::State(_));
        all.push(event);
        if last {
            // The state event ends it: nothing follows, a second state event least of all.
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(
                events.try_recv().is_err(),
                "something after the state event"
            );
            return all;
        }
    }
}
