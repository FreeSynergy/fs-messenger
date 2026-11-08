//! The SDK's own conformance suite for crypto stores, over `FsCryptoStore`. Each `get_store` with
//! `clear_data = false` builds a new store from the journal, so what the suite reads back after such a
//! call has gone through an encrypted `TursoStore`.
// The macro (matrix-sdk-crypto) emits its own `#[ignore]`s without a reason.
#![allow(clippy::ignore_without_reason)]
// The SDK's macro asks for features of its own crate, which this crate does not have.
#![allow(unexpected_cfgs)]

mod support;

use std::sync::Arc;

use fs_messenger_matrix::store::{FsCryptoStore, FsJournal};
use matrix_sdk_crypto::cryptostore_integration_tests;

async fn get_store(name: &str, _passphrase: Option<&str>, clear_data: bool) -> FsCryptoStore {
    let store = support::named_store(name, clear_data).await;
    let journal = FsJournal::open(store, "crypto").await.unwrap();
    FsCryptoStore::open(Arc::new(journal)).await.unwrap()
}

cryptostore_integration_tests!();
