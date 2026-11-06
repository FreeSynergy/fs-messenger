//! ADR 0018, 0024, 0029: the program does not open on a plain store, tested with real stores.

mod common;

use std::sync::Arc;

use common::{ME, account, raw, repo_over};
use fs_messenger_core::{Error, Ingest, Ingested, Messenger, MessengerConfig, MessengerRepo};
use fs_store::{
    ActivityFilter, Cipher, EncryptionKey, OpenOptions, Query, StaticKeyProvider, Store, TursoStore,
};

fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

fn config() -> MessengerConfig {
    MessengerConfig::new(ME)
}

#[tokio::test]
async fn a_plain_store_is_refused_and_gets_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Arc::new(TursoStore::open(dir.path().join("plain.db")).await.unwrap());
    let result = MessengerRepo::open(plain.clone(), config()).await;
    assert!(matches!(result, Err(Error::StoreNotEncrypted)));
    assert_eq!(plain.count(&Query::new()).await.unwrap(), 0);
    let log = plain.activities(&ActivityFilter::default()).await.unwrap();
    assert!(log.is_empty());
}

#[tokio::test]
async fn open_turso_also_refuses_a_store_that_opted_out_of_the_rule() {
    let dir = tempfile::tempdir().unwrap();
    let opted_out = OpenOptions {
        allow_sensitive_unencrypted: true,
        ..OpenOptions::default()
    };
    let store = TursoStore::open_with(dir.path().join("p.db"), opted_out)
        .await
        .unwrap();
    let store = Arc::new(store);
    let result = MessengerRepo::open_turso(store.clone(), config()).await;
    assert!(matches!(result, Err(Error::StoreNotEncrypted)));

    // The documented limit: behind `dyn Store` the opted-out store cannot be told apart. See DESIGN.md.
    assert!(MessengerRepo::open(store, config()).await.is_ok());
}

#[tokio::test]
async fn an_encrypted_store_opens_keeps_no_plaintext_and_reopens_without_growing_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("enc.db");
    let needle = "very-secret-needle-0xBEEF";
    let id = {
        let store = TursoStore::open_with(&path, encrypted()).await.unwrap();
        let repo = MessengerRepo::open_turso(Arc::new(store), config())
            .await
            .unwrap();
        let me = account(&repo, "me@example.org").await;
        let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", needle)).await.unwrap() else {
            panic!();
        };
        id
    };
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let hit = bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
        assert!(!hit, "the text must not be readable on disk");
    }

    let store = Arc::new(TursoStore::open_with(&path, encrypted()).await.unwrap());
    let before = store
        .activities(&ActivityFilter::default())
        .await
        .unwrap()
        .len();
    let repo = MessengerRepo::open_turso(store.clone(), config())
        .await
        .unwrap();
    let after = store
        .activities(&ActivityFilter::default())
        .await
        .unwrap()
        .len();
    assert_eq!(before, after, "the check writes once, not at every start");

    let page = repo.conversations(None, false, None).await.unwrap();
    let got = repo.message(page.items[0].id, id).await.unwrap();
    assert_eq!(got.body_text, needle);
}

#[tokio::test]
async fn everything_the_program_stores_is_marked_sensitive() {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let repo = repo_over(store.clone(), 50).await;
    let (me, _) = common::with_provider(&repo, fs_messenger_core::Encryption::Mls, &["b@x"]).await;
    let mut m = raw("m1", "t", "x");
    m.attachments.push(fs_messenger_core::RawAttachment {
        name: "a.txt".into(),
        media_type: "text/plain".into(),
        data: b"file".to_vec(),
    });
    repo.ingest(me.id, m).await.unwrap();
    let mut d = repo.compose(None).await.unwrap();
    d.set_text("draft".into(), "text/plain".into())
        .await
        .unwrap();
    d.save().await.unwrap();
    let mut sent = repo.compose(None).await.unwrap();
    sent.set_recipients(vec!["b@x".into()], vec![], vec![])
        .await
        .unwrap();
    sent.set_text("out".into(), "text/plain".into())
        .await
        .unwrap();
    sent.send(None).await.unwrap();

    let all = store.query(&Query::new()).await.unwrap();
    let types: Vec<_> = all
        .iter()
        .map(|o| o.envelope.object_type.as_str())
        .collect();
    for t in [
        "Note",
        "Collection",
        "Document",
        "freesynergy.net:Account",
        "freesynergy.net:Draft",
    ] {
        assert!(types.contains(&t), "{t} in {types:?}");
    }
    for o in &all {
        assert!(
            !o.envelope.sensitive.is_empty(),
            "{}",
            o.envelope.object_type
        );
    }
}

#[tokio::test]
async fn the_stores_own_refusal_holds_for_a_message_built_by_the_program() {
    // Written through a repo on an opted-out store, then offered to a plain one: refused there too.
    let dir = tempfile::tempdir().unwrap();
    let opted_out = OpenOptions {
        allow_sensitive_unencrypted: true,
        ..OpenOptions::default()
    };
    let source = Arc::new(
        TursoStore::open_with(dir.path().join("a.db"), opted_out)
            .await
            .unwrap(),
    );
    let repo = MessengerRepo::open(source.clone(), config()).await.unwrap();
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let object = source.get(id).await.unwrap().unwrap();
    let plain = TursoStore::open(dir.path().join("b.db")).await.unwrap();
    let refused = plain.put(object, ME).await;
    assert!(matches!(
        refused,
        Err(fs_store::Error::EncryptionRequired(_))
    ));
}
