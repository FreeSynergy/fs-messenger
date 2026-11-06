//! The SDK's own conformance suite for state stores, over `FsStateStore`.

mod support;

use std::sync::Arc;

use fs_messenger_matrix::StoreFail;
use fs_messenger_matrix::store::{FsJournal, FsStateStore};
use matrix_sdk_base::statestore_integration_tests;

async fn get_store() -> Result<FsStateStore, StoreFail> {
    let store = support::fresh_store().await;
    let journal = FsJournal::open(store, "state").await?;
    FsStateStore::open(Arc::new(journal)).await
}

statestore_integration_tests!();
