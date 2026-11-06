//! N17: what stays of a message over the limit, so it is visible and not gone (ADR 0030).

mod common;

use std::sync::Arc;

use common::{ME, account, alias, conversation_of, raw};
use fs_messenger_core::{
    Encryption, Error, FetchBatch, Ingest, Ingested, MessageAction, Messenger, MessengerConfig,
    MessengerRepo, ProviderCursor, RawAttachment, SearchFilter,
};
use fs_store::{Blob, Store, TursoStore};

async fn limited(limit: usize) -> MessengerRepo {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let mut config = MessengerConfig::new(ME);
    config.max_message_bytes = limit;
    MessengerRepo::open(store, config).await.unwrap()
}

#[tokio::test]
async fn n17_what_stays_of_a_mail_too_large_is_visible_and_has_no_content() {
    let repo = limited(100).await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("big", "t", &"secret text ".repeat(50));
    m.subject = Some("Holiday photos".into());
    m.published = Some(common::at(3));
    m.original = Some(Blob {
        media_type: "message/rfc822".into(),
        bytes: vec![1; 5000],
    });
    m.attachments.push(RawAttachment {
        name: "photo".into(),
        media_type: "image/png".into(),
        data: vec![0; 5000],
    });
    let got = repo.ingest(me.id, m.clone()).await.unwrap();
    let Ingested::TooLarge { id, size, limit } = got else {
        panic!("expected the named refusal: {got:?}");
    };
    assert_eq!(limit, 100);
    assert!(size > 5000);

    let conv = conversation_of(&repo).await;
    let stub = repo.message(conv, id).await.unwrap();
    assert_eq!(stub.too_large, Some(size as u64));
    assert_eq!(stub.aliases, [alias("big")]);
    assert_eq!(
        stub.summary.envelope.attributed_to.as_deref(),
        Some("ada@example.org")
    );
    assert_eq!(stub.summary.envelope.published, common::at(3));
    assert_eq!(stub.summary.envelope.name, "Holiday photos");
    assert!(stub.summary.unread, "it is there to be seen");
    assert!(stub.body_text.is_empty() && stub.original.is_none() && stub.attachments.is_empty());
    assert_eq!(stub.summary.preview.len(), 0);
    let filter = SearchFilter::default();
    assert_eq!(
        repo.search("secret", &filter, None)
            .await
            .unwrap()
            .items
            .len(),
        0
    );

    assert_eq!(
        repo.ingest(me.id, m).await.unwrap(),
        Ingested::Duplicate(id),
        "a replay"
    );
    let forward = MessageAction::Forward(vec!["b@x".into()]);
    assert!(matches!(
        repo.act(conv, id, forward).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn n17_a_sync_counts_it_as_rejected_keeps_the_stub_and_goes_on() {
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
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.messages(conv, None).await.unwrap().items.len(), 2);
}
