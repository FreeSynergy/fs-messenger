//! Helpers of the offline tests: the real provider, real keys and a real store; only the wire is
//! replaced, by a [`Wire`] that hands over bytes given by the test.

#![allow(dead_code)] // each test file uses a different subset

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_messenger_mail::fetch::{
    FetchedBatch, FetchedMail, MailCursor, MailFetch, MailboxCursor, MailboxRole,
};
use fs_messenger_mail::keys::{FsKeyStore, KeyStore};
use fs_messenger_mail::submit::{MailEnvelope, MailSubmit, SentCopy};
use fs_messenger_mail::{MailError, MailIdentity, MailProvider};
use fs_store::{Store, TursoStore};

/// Mails to deliver, once each, and what was submitted.
#[derive(Default)]
pub struct Wire {
    pub inbox: Mutex<Vec<Vec<u8>>>,
    pub sent: Mutex<Vec<(MailEnvelope, Vec<u8>)>>,
}

#[async_trait]
impl MailFetch for Wire {
    async fn fetch(&self, cursor: &MailCursor, _limit: usize) -> Result<FetchedBatch, MailError> {
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
        self.sent.lock().unwrap().push((envelope.clone(), raw.to_vec()));
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

pub async fn person(address: &str) -> Offline {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let keys: Arc<dyn KeyStore> = Arc::new(FsKeyStore::open(store, "https://local.invalid/me"));
    let wire = Arc::new(Wire::default());
    let provider = Arc::new(MailProvider::new(
        MailIdentity::new(address),
        wire.clone(),
        wire.clone(),
        keys.clone(),
    ));
    provider.ensure_own_key().await.unwrap();
    Offline {
        address: address.to_string(),
        provider,
        wire,
        keys,
    }
}
