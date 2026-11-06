//! [`MessengerRepo`]: the program's entry, and what its parts share.
//!
//! Pattern: Facade + Registry: one small type hides the store, the rules and the providers;
//! providers are registered per account at runtime (hot plugins, ADR 0045). There is no way to get a
//! `MessengerRepo` without the store check.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use fs_store::{ObjectId, Store, StoredObject, TursoStore};
use tokio::sync::Mutex;

use super::store_check;
use crate::error::{Error, Result};
use crate::provider::Provider;

const DEFAULT_PAGE_SIZE: usize = 50;

/// How the program runs.
#[derive(Clone, Debug)]
pub struct MessengerConfig {
    /// Actor IRI of the person this program belongs to.
    pub actor: String,
    /// Host for object IRIs, `https://<instance>/messages/<id>` (ADR 0027), if any.
    pub instance: Option<String>,
    /// Items per page. The contract has no page size; provisional.
    pub page_size: usize,
}

impl MessengerConfig {
    /// A configuration for `actor`.
    pub fn new(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            instance: None,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

/// The store-backed messenger.
#[derive(Clone)]
pub struct MessengerRepo {
    pub(super) inner: Arc<Inner>,
}

pub(super) struct Inner {
    pub(super) store: Arc<dyn Store>,
    pub(super) config: MessengerConfig,
    /// One read-modify-write at a time. Never held while a provider runs.
    pub(super) writes: Arc<Mutex<()>>,
    providers: RwLock<HashMap<ObjectId, Arc<dyn Provider>>>,
}

impl MessengerRepo {
    /// Open on any store. Refused with `StoreNotEncrypted` if the store refuses sensitive objects.
    /// A store that was told to accept them anyway cannot be told apart behind `dyn Store`:
    /// see [`MessengerRepo::open_turso`] and `DESIGN.md`.
    pub async fn open(store: Arc<dyn Store>, config: MessengerConfig) -> Result<Self> {
        store_check::check(store.as_ref(), &config.actor).await?;
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                config,
                writes: Arc::new(Mutex::new(())),
                providers: RwLock::new(HashMap::new()),
            }),
        })
    }

    /// Open on a Turso store, which can be asked directly. Refuses every unencrypted one, also one
    /// that opted out of the sensitive-data rule. The form a program binary should use.
    pub async fn open_turso(store: Arc<TursoStore>, config: MessengerConfig) -> Result<Self> {
        if !store.is_encrypted() {
            return Err(Error::StoreNotEncrypted);
        }
        Self::open(store, config).await
    }

    /// Use `provider` for `account`, replacing any earlier one.
    pub fn register_provider(&self, account: ObjectId, provider: Arc<dyn Provider>) {
        self.inner
            .providers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(account, provider);
    }
}

impl Inner {
    pub(super) fn provider(&self, account: ObjectId) -> Option<Arc<dyn Provider>> {
        self.providers
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&account)
            .cloned()
    }

    pub(super) fn unregister(&self, account: ObjectId) {
        self.providers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&account);
    }

    /// Object IRI, if this program has an instance.
    pub(super) fn iri(&self, id: ObjectId) -> Option<String> {
        self.config
            .instance
            .as_ref()
            .map(|host| format!("https://{host}/messages/{id}"))
    }

    pub(super) async fn put(&self, object: StoredObject) -> Result<()> {
        self.store.put(object, &self.config.actor).await?;
        Ok(())
    }
}
