//! Shared test helpers: a real store, and a small test double for `Provider`.
#![allow(dead_code)] // each test file uses a different subset

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use fs_interfaces::Capabilities;
use fs_messenger_core::{
    Account, AccountSetup, Encryption, EncryptionPlan, Error, Event, ExternalId, FetchBatch, Kind,
    Listener, Messenger, MessengerConfig, MessengerRepo, Outgoing, Provider, ProviderCursor,
    RawMessage, Result, SendReceipt,
};
use fs_store::{ObjectId, Store, TursoStore};
use tokio::sync::mpsc;

pub const ME: &str = "https://local.invalid/users/owner";

pub async fn setup() -> (MessengerRepo, Arc<dyn Store>) {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    (repo_over(store.clone(), 50).await, store)
}

pub async fn repo_over(store: Arc<dyn Store>, page_size: usize) -> MessengerRepo {
    let mut config = MessengerConfig::new(ME);
    config.page_size = page_size;
    MessengerRepo::open(store, config).await.unwrap()
}

pub async fn account(repo: &MessengerRepo, address: &str) -> Account {
    repo.add_account(AccountSetup {
        address: address.into(),
        kind: "mail".into(),
        secret: String::new(),
    })
    .await
    .unwrap()
}

/// With a provider that knows keys for `keys`.
pub async fn with_provider(
    repo: &MessengerRepo,
    encryption: Encryption,
    keys: &[&str],
) -> (Account, Arc<TestProvider>) {
    let account = account(repo, "me@example.org").await;
    let provider = TestProvider::new(encryption, keys);
    repo.register_provider(account.id, provider.clone());
    (account, provider)
}

pub fn alias(id: &str) -> ExternalId {
    ExternalId::new("uid", id)
}

pub fn message_id(id: &str) -> ExternalId {
    ExternalId::new(fs_messenger_core::MESSAGE_ID, id)
}

pub fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, minute, 0).unwrap()
}

/// An incoming message in thread `thread`, known to be OpenPGP.
pub fn raw(id: &str, thread: &str, text: &str) -> RawMessage {
    let mut m = RawMessage::new(
        alias(id),
        ExternalId::new("thread", thread),
        "ada@example.org",
        text,
    );
    m.encryption = Encryption::Openpgp;
    m.kind = Kind::Mail;
    m
}

pub fn raw_at(id: &str, thread: &str, minute: u32) -> RawMessage {
    let mut m = raw(id, thread, id);
    m.published = Some(at(minute));
    m
}

/// A test double: plans from a list of known keys, and records what it was handed.
pub struct TestProvider {
    pub encryption: Mutex<Encryption>,
    pub keys: Mutex<Vec<String>>,
    pub sent: Mutex<Vec<Outgoing>>,
    pub batches: Mutex<VecDeque<FetchBatch>>,
    pub since: Mutex<Vec<Option<ProviderCursor>>>,
    pub receipt: Mutex<SendReceipt>,
    pub fail_send: Mutex<bool>,
    /// Always answers the same batch, which claims there is more.
    pub stuck: Mutex<bool>,
}

impl TestProvider {
    pub fn new(encryption: Encryption, keys: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            encryption: Mutex::new(encryption),
            keys: Mutex::new(keys.iter().map(ToString::to_string).collect()),
            sent: Mutex::new(vec![]),
            batches: Mutex::new(VecDeque::new()),
            since: Mutex::new(vec![]),
            receipt: Mutex::new(SendReceipt {
                aliases: vec![alias("sent-1")],
                conversation: None,
                encryption,
            }),
            fail_send: Mutex::new(false),
            stuck: Mutex::new(false),
        })
    }

    pub fn sent_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

#[async_trait]
impl Provider for TestProvider {
    fn kind(&self) -> Result<String> {
        Ok("mail".into())
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::new("test");
        c.supported("encryption.plan");
        c
    }

    async fn fetch(&self, since: Option<&ProviderCursor>) -> Result<FetchBatch> {
        self.since.lock().unwrap().push(since.cloned());
        if *self.stuck.lock().unwrap() {
            return Ok(FetchBatch {
                messages: vec![],
                next: since.cloned(),
                more: true,
            });
        }
        Ok(self.batches.lock().unwrap().pop_front().unwrap_or_default())
    }

    async fn send(&self, message: &Outgoing) -> Result<SendReceipt> {
        if *self.fail_send.lock().unwrap() {
            return Err(Error::Provider("the line is down".into()));
        }
        self.sent.lock().unwrap().push(message.clone());
        Ok(self.receipt.lock().unwrap().clone())
    }

    async fn encryption_plan(&self, recipients: &[String]) -> Result<EncryptionPlan> {
        let keys = self.keys.lock().unwrap();
        Ok(EncryptionPlan {
            encryption: *self.encryption.lock().unwrap(),
            missing_keys: recipients
                .iter()
                .filter(|r| !keys.contains(r))
                .cloned()
                .collect(),
        })
    }
}

/// A listener that forwards what it hears.
pub struct Collector(pub mpsc::UnboundedSender<Event>);

#[async_trait]
impl Listener for Collector {
    async fn event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

pub fn collector() -> (Arc<Collector>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Arc::new(Collector(tx)), rx)
}

pub async fn next_event(rx: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event in time")
        .expect("the channel is open")
}

/// Silence: nothing in time, or the channel closed because the subscription ended.
pub async fn no_event(rx: &mut mpsc::UnboundedReceiver<Event>) {
    let got = tokio::time::timeout(Duration::from_millis(150), rx.recv()).await;
    if let Ok(Some(event)) = got {
        panic!("expected silence, got {event:?}");
    }
}

/// The newest conversation.
pub async fn conversation_of(repo: &MessengerRepo) -> ObjectId {
    let page = repo.conversations(None, false, None).await.unwrap();
    page.items.first().expect("a conversation").id
}
