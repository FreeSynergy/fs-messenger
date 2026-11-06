//! C24: size limits at ingest, and how far search looks. Provisional values.

mod common;

use std::sync::Arc;

use common::{ME, account, conversation_of, raw};
use fs_messenger_core::{
    Encryption, Error, FetchBatch, Ingest, Ingested, Messenger, MessengerConfig, MessengerRepo,
    ProviderCursor, RawAttachment, SearchFilter,
};
use fs_store::{Store, TursoStore};

async fn limited(limit: usize) -> MessengerRepo {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let mut config = MessengerConfig::new(ME);
    config.max_message_bytes = limit;
    MessengerRepo::open(store, config).await.unwrap()
}

#[test]
fn the_default_limit_is_25_mib() {
    assert_eq!(MessengerConfig::new(ME).max_message_bytes, 25 * 1024 * 1024);
}

#[tokio::test]
async fn a_message_over_the_limit_is_refused_by_name_with_all_parts_counted() {
    let repo = limited(100).await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("big", "t", "small text");
    m.attachments.push(RawAttachment {
        name: "a".into(),
        media_type: "application/x-a".into(),
        data: vec![0; 101],
    });
    let Err(Error::TooLarge { size, limit }) = repo.ingest(me.id, m).await else {
        panic!("expected the named refusal");
    };
    assert_eq!((size, limit), (111, 100));
    assert!(
        repo.conversations(None, false, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn a_sync_counts_it_as_rejected_and_goes_on() {
    let repo = limited(100).await;
    let me = account(&repo, "me@example.org").await;
    let provider = common::TestProvider::new(Encryption::Mls, &[]);
    repo.register_provider(me.id, provider.clone());
    provider.batches.lock().unwrap().push_back(FetchBatch {
        messages: vec![raw("big", "t", &"x".repeat(101)), raw("ok", "t", "fine")],
        next: Some(ProviderCursor("c1".into())),
        more: false,
    });
    let report = repo.sync(me.id).await.unwrap();
    assert_eq!((report.stored, report.rejected), (1, 1));
}

#[tokio::test]
async fn search_stops_at_256_kib_and_the_message_stays_whole() {
    let repo = limited(10 * 1024 * 1024).await;
    let me = account(&repo, "me@example.org").await;
    let text = format!("nearstart {} farbeyond", "ä".repeat(150 * 1024));
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m", "t", &text)).await.unwrap() else {
        panic!();
    };
    let filter = SearchFilter::default();
    assert_eq!(
        repo.search("nearstart", &filter, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(
        repo.search("farbeyond", &filter, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().body_text, text);
}
