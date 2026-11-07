//! The harness: one person with real encrypted stores, the core's `Draft.send` and a real server.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use fs_messenger_core::{
    AccountSetup, Ingest, Message, Messenger, MessengerConfig, MessengerRepo, SearchFilter,
};
use fs_messenger_mail::fetch::{FetchLimits, FetchedBatch, MailCursor, MailFetch};
use fs_messenger_mail::jmap::{JmapFetch, JmapSubmit};
use fs_messenger_mail::keys::{FsKeyStore, KeyStore};
use fs_messenger_mail::submit::MailSubmit;
use fs_messenger_mail::{MailError, MailIdentity, MailProvider};
use fs_objects::{Cipher, EncryptionKey, ObjectId, OpenOptions, StaticKeyProvider, TursoStore};

pub use crate::env::{Who, serial};
pub use crate::wire::{contains, message_id_of, raw_by_message_id, raw_mails, smtp_raw, token};
use crate::wire::{imap_fetch, jmap_client, smtp_submit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Imap,
    Jmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    Smtp,
    Jmap,
}

/// Decorator over a real strategy: counts what the server really delivered.
pub struct Counting {
    inner: Arc<dyn MailFetch>,
    pub delivered: AtomicUsize,
}

#[async_trait]
impl MailFetch for Counting {
    async fn fetch(
        &self,
        cursor: &MailCursor,
        limits: &FetchLimits,
    ) -> Result<FetchedBatch, MailError> {
        let batch = self.inner.fetch(cursor, limits).await?;
        self.delivered
            .fetch_add(batch.mails.len(), Ordering::SeqCst);
        Ok(batch)
    }
    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError> {
        self.inner.store_sent(raw).await
    }
}

fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

/// One person with an encrypted store on disk, the core and a provider on the real server.
pub struct Person {
    pub who: Who,
    pub repo: MessengerRepo,
    pub account: ObjectId,
    pub provider: Arc<MailProvider>,
    pub counting: Arc<Counting>,
    pub dir: PathBuf,
    pub keys: Arc<FsKeyStore>,
    pub message_store: Arc<TursoStore>,
    pub key_store: Arc<TursoStore>,
}

impl Person {
    pub async fn open(who: Who, dir: &Path, fetch: Fetch, submit: Submit) -> Self {
        Self::open_provider_limited(who, dir, fetch, submit, 25 * 1024 * 1024).await
    }

    /// With this limit on a mail in the provider; the core keeps its own default.
    pub async fn open_provider_limited(
        who: Who,
        dir: &Path,
        fetch: Fetch,
        submit: Submit,
        limit: usize,
    ) -> Self {
        let message_store = Arc::new(
            TursoStore::open_with(dir.join("messages.db"), encrypted())
                .await
                .unwrap(),
        );
        let actor = format!("https://local.invalid/users/{}", who.address());
        let config = MessengerConfig::new(actor.clone());
        let repo = MessengerRepo::open_turso(message_store.clone(), config)
            .await
            .unwrap();
        // ADR 0018: keys in a store of their own, encrypted; never synchronised with the messages.
        let key_store = Arc::new(
            TursoStore::open_with(dir.join("keys.db"), encrypted())
                .await
                .unwrap(),
        );
        let keys = Arc::new(FsKeyStore::open_turso(key_store.clone(), actor).unwrap());
        let address = who.address();
        let account = match repo.accounts().await.unwrap().pop() {
            Some(a) => a,
            None => repo
                .add_account(AccountSetup {
                    address: address.clone(),
                    kind: "mail".into(),
                    secret: String::new(),
                })
                .await
                .unwrap(),
        };
        let client = jmap_client(who);
        let fetcher: Arc<dyn MailFetch> = match fetch {
            Fetch::Imap => Arc::new(imap_fetch(who)),
            Fetch::Jmap => Arc::new(JmapFetch::new(client.clone())),
        };
        let counting = Arc::new(Counting {
            inner: fetcher,
            delivered: AtomicUsize::new(0),
        });
        let submitter: Arc<dyn MailSubmit> = match submit {
            Submit::Smtp => Arc::new(smtp_submit(who)),
            Submit::Jmap => Arc::new(JmapSubmit::new(client)),
        };
        let provider = Arc::new(
            MailProvider::new(
                MailIdentity::new(&address),
                counting.clone(),
                submitter,
                keys.clone() as Arc<dyn KeyStore>,
            )
            .with_max_message_bytes(limit),
        );
        provider.ensure_own_key().await.unwrap();
        repo.register_provider(account.id, provider.clone());
        Self {
            who,
            repo,
            account: account.id,
            provider,
            counting,
            dir: dir.to_path_buf(),
            keys,
            message_store,
            key_store,
        }
    }

    /// `self` learns the other's key by hand, the first contact when nothing leaves unencrypted.
    pub async fn knows(&self, other: &Person) {
        let cert = other.provider.ensure_own_key().await.unwrap().public();
        self.provider
            .import_peer_key(&other.who.address(), &cert)
            .await
            .unwrap();
    }

    /// Send through the core: `Draft.send`.
    pub async fn send(
        &self,
        reply_to: Option<ObjectId>,
        to: &[String],
        bcc: &[String],
        text: &str,
        files: &[(&str, &str, &[u8])],
    ) -> fs_messenger_core::Result<ObjectId> {
        let mut draft = self.repo.compose(reply_to).await?;
        // A reply goes out from its conversation's account; the core refuses a second opinion.
        if reply_to.is_none() {
            draft.set_account(self.account).await?;
        }
        draft
            .set_recipients(to.to_vec(), vec![], bcc.to_vec())
            .await?;
        // The tests write the subject as the first line, so that one needle finds both.
        draft
            .set_subject(text.lines().next().unwrap_or("").into())
            .await?;
        draft.set_text(text.into(), "text/plain".into()).await?;
        for (name, media, data) in files {
            draft
                .attach((*name).to_string(), (*media).to_string(), data.to_vec())
                .await?;
        }
        draft.send(None).await
    }

    /// Sync until a message with `needle` is here.
    pub async fn wait_for(&self, needle: &str) -> Message {
        for _ in 0..60 {
            self.repo.sync(self.account).await.expect("sync");
            if let Some(m) = self.find(needle).await.pop() {
                return m;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("no message with {needle} within 30 s");
    }

    /// Everything stored with `needle`.
    pub async fn find(&self, needle: &str) -> Vec<Message> {
        let filter = SearchFilter::default();
        let page = self
            .repo
            .search(needle, &filter, None)
            .await
            .expect("search");
        let mut found = vec![];
        for s in page.items {
            let conversation = s.envelope.context.as_deref().unwrap().parse().unwrap();
            found.push(
                self.repo
                    .message(conversation, s.envelope.id)
                    .await
                    .unwrap(),
            );
        }
        found
    }

    pub async fn message(&self, id: ObjectId) -> Message {
        let page = self.repo.conversations(None, false, None).await.unwrap();
        for c in page.items {
            if let Ok(m) = self.repo.message(c.id, id).await {
                return m;
            }
        }
        panic!("message {id} not found");
    }
}
