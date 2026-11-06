//! An encrypted store in a temporary file, for the store tests. Never an in-memory or plain one: the
//! journal refuses those.
#![allow(dead_code)] // each test file uses a different subset

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};
use tempfile::TempDir;

pub mod server;

/// Open the encrypted store at `path`.
pub async fn open_at(path: &Path) -> TursoStore {
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    };
    TursoStore::open_with(path.to_str().unwrap(), options)
        .await
        .unwrap()
}

/// A new encrypted store; the directory lives as long as the test process.
pub async fn fresh_store() -> Arc<TursoStore> {
    let dir = TempDir::new().unwrap();
    let store = open_at(&dir.path().join("store.db")).await;
    std::mem::forget(dir);
    Arc::new(store)
}

/// The store of `name`, kept between calls. With `clear`, a new one.
pub async fn named_store(name: &str, clear: bool) -> Arc<TursoStore> {
    static STORES: OnceLock<Mutex<HashMap<String, Arc<TursoStore>>>> = OnceLock::new();
    let stores = STORES.get_or_init(Mutex::default);
    if !clear && let Some(store) = stores.lock().unwrap().get(name) {
        return store.clone();
    }
    let store = fresh_store().await;
    stores
        .lock()
        .unwrap()
        .insert(name.to_owned(), store.clone());
    store
}
