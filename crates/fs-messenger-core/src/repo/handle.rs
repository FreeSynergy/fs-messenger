//! [`MessengerRepo`]: the program's entry, and what its parts share.
//!
//! Pattern: Facade + Registry: one small type hides the store, the rules and the providers;
//! providers are registered per account at runtime (hot plugins, ADR 0045). Unless a build asks for
//! `test-support`, the one way to a `MessengerRepo` is `open_turso`, which asks whether the store is
//! encrypted.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as Mutex2, PoisonError, RwLock};

use fs_objects::{ObjectId, Query, Store, StoredObject, TursoStore};
use tokio::sync::Mutex;

use super::consts::IDX_ARRIVAL_DESC;
use super::keys::arrival_of_desc_key;
use super::store_check;
use crate::error::{Error, Result};
use crate::provider::Provider;

const DEFAULT_PAGE_SIZE: usize = 50;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 25 * 1024 * 1024;

/// How the program runs.
#[derive(Clone, Debug)]
pub struct MessengerConfig {
    /// Actor IRI of the person this program belongs to.
    pub actor: String,
    /// Host for object IRIs, `https://<instance>/messages/<id>` (ADR 0027), if any.
    pub instance: Option<String>,
    /// Items per page. The contract has no page size; provisional.
    pub page_size: usize,
    /// Largest message taken from a provider, all its parts counted. Provisional.
    pub max_message_bytes: usize,
}

impl MessengerConfig {
    /// A configuration for `actor`.
    pub fn new(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            instance: None,
            page_size: DEFAULT_PAGE_SIZE,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
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
    /// Highest local arrival sequence handed out.
    arrival: AtomicU64,
    _handle: StoreHandle,
    providers: RwLock<HashMap<ObjectId, Arc<dyn Provider>>>,
}

impl MessengerRepo {
    /// Open on a Turso store. Refuses every unencrypted one, including one that opted out of the
    /// sensitive-data rule. The only constructor, unless a build asks for `test-support`.
    ///
    /// Keep one handle per store and clone it: a second one would share no write lock and no arrival
    /// counter. A second open of the same `Arc` is refused with `Conflict`. A second `TursoStore` on
    /// the same file is another object, which this cannot see; `DESIGN.md` says so.
    pub async fn open_turso(store: Arc<TursoStore>, config: MessengerConfig) -> Result<Self> {
        if !store.is_encrypted() {
            return Err(Error::StoreNotEncrypted);
        }
        Self::checked(store, config).await
    }

    /// Open on any store, for tests only: a plain in-memory store passes, because it accepts
    /// sensitive objects. A store that refuses them is still refused.
    #[cfg(feature = "test-support")]
    pub async fn open(store: Arc<dyn Store>, config: MessengerConfig) -> Result<Self> {
        Self::checked(store, config).await
    }

    async fn checked(store: Arc<dyn Store>, config: MessengerConfig) -> Result<Self> {
        store_check::check(store.as_ref(), &config.actor).await?;
        let handle = StoreHandle::claim(&store)?;
        let arrival = AtomicU64::new(highest_arrival(store.as_ref()).await?);
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                config,
                writes: Arc::new(Mutex::new(())),
                arrival,
                _handle: handle,
                providers: RwLock::new(HashMap::new()),
            }),
        })
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

/// The stores open in this process, by address.
static OPEN_STORES: Mutex2<BTreeSet<usize>> = Mutex2::new(BTreeSet::new());

/// RAII guard: the claim on a store ends with the last clone of its repo. `_handle` is declared after
/// `store` in `Inner`, so it drops after it: for a moment the address is claimed though the store has
/// gone. A new store at that address is then refused, which fails safe. A provider holding a repo
/// clone keeps the claim for as long as it is registered; see `DESIGN.md`.
struct StoreHandle(usize);

impl StoreHandle {
    fn claim(store: &Arc<dyn Store>) -> Result<Self> {
        let address = Arc::as_ptr(store).cast::<()>() as usize;
        let mut open = OPEN_STORES.lock().unwrap_or_else(PoisonError::into_inner);
        if !open.insert(address) {
            return Err(Error::Conflict(
                "this store is already open; clone the repo instead".into(),
            ));
        }
        Ok(Self(address))
    }
}

impl Drop for StoreHandle {
    fn drop(&mut self) {
        let mut open = OPEN_STORES.lock().unwrap_or_else(PoisonError::into_inner);
        open.remove(&self.0);
    }
}

/// Tombstones keep their sequence too, so a number is never handed out twice.
async fn highest_arrival(store: &dyn Store) -> Result<u64> {
    let q = Query::new().order_by_index(IDX_ARRIVAL_DESC).limit(1);
    let newest = store.query(&q).await?;
    Ok(newest
        .first()
        .and_then(|o| o.index.get(IDX_ARRIVAL_DESC))
        .and_then(|v| v.first())
        .and_then(|k| arrival_of_desc_key(k))
        .unwrap_or(0))
}

impl Inner {
    /// The next local arrival sequence. Call while holding `writes`.
    pub(super) fn next_arrival(&self) -> u64 {
        self.arrival.fetch_add(1, Ordering::Relaxed) + 1
    }

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
