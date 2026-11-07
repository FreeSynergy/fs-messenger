//! Helpers of the offline tests: the real provider, real keys and a real store; only the wire is
//! replaced, by a [`Wire`] that hands over bytes given by the test.

#![allow(dead_code)] // each test file uses a different subset

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_messenger_core::{
    AccountSetup, Messenger as _, MessengerConfig, MessengerRepo, Provider as _, RawMessage,
};
use fs_messenger_mail::fetch::{
    FetchLimits, FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole,
};
use fs_messenger_mail::keys::{FsKeyStore, KeyStore, PeerKey};
use fs_messenger_mail::openpgp::SecretCert;
use fs_messenger_mail::submit::{MailEnvelope, MailSubmit, SentCopy};
use fs_messenger_mail::{MailError, MailIdentity, MailProvider};
use fs_objects::{ObjectId, Store, TursoStore};

/// Mails to deliver, once each, and what was submitted.
#[derive(Default)]
pub struct Wire {
    pub inbox: Mutex<Vec<Vec<u8>>>,
    pub sent: Mutex<Vec<(MailEnvelope, Vec<u8>)>>,
    /// What is delivered counts as only the start of a mail over the limit, of this size.
    pub cut_off: AtomicU64,
}

#[async_trait]
impl MailFetch for Wire {
    async fn fetch(
        &self,
        cursor: &MailCursor,
        _limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError> {
        let mails: Vec<Vec<u8>> = std::mem::take(&mut *self.inbox.lock().unwrap());
        let last = match cursor.mailboxes.get("box") {
            Some(MailboxCursor::Imap { last_uid, .. }) => *last_uid,
            _ => 0,
        };
        let mut next = cursor.clone();
        let count = u32::try_from(mails.len()).unwrap();
        next.mailboxes.insert(
            "box".into(),
            MailboxCursor::Imap {
                uid_validity: 1,
                last_uid: last + count,
            },
        );
        let delivered = mails.into_iter().enumerate().map(|(i, bytes)| FetchedMail {
            bytes,
            role: MailboxRole::Inbox,
            stable_id: format!("box/1/{}", last + 1 + u32::try_from(i).unwrap()),
            too_large: Some(self.cut_off.load(Ordering::SeqCst)).filter(|size| *size > 0),
        });
        Ok(FetchedBatch {
            mails: delivered.collect(),
            cursor: next,
            more: false,
        })
    }

    async fn store_sent(&self, _raw: &[u8]) -> Result<(), MailError> {
        Ok(())
    }
}

#[async_trait]
impl MailSubmit for Wire {
    async fn submit(&self, envelope: &MailEnvelope, raw: &[u8]) -> Result<SentCopy, MailError> {
        self.sent
            .lock()
            .unwrap()
            .push((envelope.clone(), raw.to_vec()));
        Ok(SentCopy::Stored)
    }
}

/// A person with a real key in a real store.
pub struct Offline {
    pub address: String,
    pub provider: Arc<MailProvider>,
    pub wire: Arc<Wire>,
    pub keys: Arc<dyn KeyStore>,
}

/// A person with a key store of their choice.
async fn person_on(address: &str, keys: Arc<dyn KeyStore>, limit: Option<usize>) -> Offline {
    let wire = Arc::new(Wire::default());
    let mut provider = MailProvider::new(
        MailIdentity::new(address),
        wire.clone(),
        wire.clone(),
        keys.clone(),
    );
    if let Some(limit) = limit {
        provider = provider.with_max_message_bytes(limit);
    }
    let provider = Arc::new(provider);
    provider.ensure_own_key().await.unwrap();
    Offline {
        address: address.to_string(),
        provider,
        wire,
        keys,
    }
}

async fn real_keys() -> Arc<dyn KeyStore> {
    let store = Arc::new(TursoStore::open_in_memory().await.unwrap());
    Arc::new(FsKeyStore::open(store, "https://local.invalid/me"))
}

pub async fn person(address: &str) -> Offline {
    person_on(address, real_keys().await, None).await
}

pub async fn person_limited(address: &str, limit: usize) -> Offline {
    person_on(address, real_keys().await, Some(limit)).await
}

/// Decorator over the real store: a lookup of a peer panics, as a bug somewhere below would.
struct PanickingPeers(Arc<dyn KeyStore>);

#[async_trait]
impl KeyStore for PanickingPeers {
    async fn own_key(&self, address: &str) -> Result<Option<SecretCert>, MailError> {
        self.0.own_key(address).await
    }
    async fn put_own_key(&self, address: &str, key: &SecretCert) -> Result<(), MailError> {
        self.0.put_own_key(address, key).await
    }
    async fn peer(&self, _address: &str) -> Result<Option<PeerKey>, MailError> {
        panic!("a bug below the reading flow");
    }
    async fn put_peer(&self, peer: &PeerKey) -> Result<(), MailError> {
        self.0.put_peer(peer).await
    }
}

pub async fn person_with_panicking_peers(address: &str) -> Offline {
    person_on(address, Arc::new(PanickingPeers(real_keys().await)), None).await
}

/// Puts a mail on the wire, for the next fetch.
pub fn wire_in(p: &Offline, raw: String) {
    p.wire.inbox.lock().unwrap().push(raw.into_bytes());
}

/// One mail through the real reading flow.
pub async fn deliver(p: &Offline, raw: &[u8]) -> RawMessage {
    p.wire.inbox.lock().unwrap().push(raw.to_vec());
    let mut batch = p.provider.fetch(None).await.unwrap();
    assert_eq!(batch.messages.len(), 1);
    batch.messages.pop().unwrap()
}

/// The real core over a real store, with this person's provider.
pub async fn core_with(p: &Offline) -> (MessengerRepo, ObjectId) {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let repo = MessengerRepo::open(store, MessengerConfig::new("https://local.invalid/me"))
        .await
        .unwrap();
    let account = repo
        .add_account(AccountSetup {
            address: p.address.clone(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    repo.register_provider(account.id, p.provider.clone());
    (repo, account.id)
}
