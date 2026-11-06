//! Delivery from a provider: dedupe on replay, sync cursor, threading out of order, original kept.

mod common;

use common::{account, alias, conversation_of, raw, raw_at, setup};
use fs_messenger_core::{Error, Ingest, Ingested, MessageAction, Messenger};
use fs_store::{Blob, Query};

#[tokio::test]
async fn a_replay_is_not_stored_twice_and_read_state_survives() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "hello")).await.unwrap() else {
        panic!("first delivery is new");
    };
    let conv = conversation_of(&repo).await;
    repo.mark_read(conv, id).await.unwrap();

    let again = repo.ingest(me.id, raw("m1", "t", "hello")).await.unwrap();
    assert_eq!(again, Ingested::Duplicate(id));

    let notes = store.count(&Query::new().of_type("Note")).await.unwrap();
    assert_eq!(notes, 1);
    assert!(!repo.message(conv, id).await.unwrap().summary.unread);
}

#[tokio::test]
async fn any_known_alias_is_enough_and_new_aliases_are_kept() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let mut both = raw("m1", "t", "x");
    both.aliases.push(alias("uid-9"));
    assert_eq!(
        repo.ingest(me.id, both).await.unwrap(),
        Ingested::Duplicate(id)
    );

    let only_new = raw("uid-9", "t", "x");
    assert_eq!(
        repo.ingest(me.id, only_new).await.unwrap(),
        Ingested::Duplicate(id)
    );
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().aliases.len(), 2);
}

#[tokio::test]
async fn parallel_deliveries_store_one() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut tasks = vec![];
    for _ in 0..8 {
        let (repo, id) = (repo.clone(), me.id);
        tasks.push(tokio::spawn(async move {
            repo.ingest(id, raw("m1", "t", "x")).await.unwrap()
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 1);
    assert_eq!(
        store
            .count(&Query::new().of_type("Collection"))
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn a_message_without_alias_is_refused() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("m1", "t", "x");
    m.aliases.clear();
    assert!(matches!(
        repo.ingest(me.id, m).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn the_same_alias_in_another_account_is_another_message() {
    let (repo, store) = setup().await;
    let a = account(&repo, "a@example.org").await;
    let b = account(&repo, "b@example.org").await;
    repo.ingest(a.id, raw("m1", "t", "x")).await.unwrap();
    let second = repo
        .ingest(b.id, raw("m1", "t", "forged id"))
        .await
        .unwrap();
    assert!(matches!(second, Ingested::Stored(_)));
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 2);
}

#[tokio::test]
async fn a_deleted_message_does_not_come_back_on_replay() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    repo.act(conv, id, MessageAction::Delete).await.unwrap();
    assert_eq!(
        repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap(),
        Ingested::Duplicate(id)
    );
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 0);
    assert!(matches!(
        repo.message(conv, id).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn the_original_is_kept_byte_for_byte_and_the_envelope_is_a_note() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let bytes = b"From: ada\r\n\r\nraw \xff bytes".to_vec();
    let mut m = raw_at("m1", "t", 3);
    m.subject = Some("Subject line".into());
    m.original = Some(Blob {
        media_type: "message/rfc822".into(),
        bytes: bytes.clone(),
    });
    let Ingested::Stored(id) = repo.ingest(me.id, m).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let got = repo.message(conv, id).await.unwrap();
    assert_eq!(got.original.unwrap().bytes, bytes);
    assert_eq!(got.original_media_type, "message/rfc822");
    let env = got.summary.envelope;
    assert_eq!(env.object_type, "Note");
    assert_eq!(env.name, "Subject line");
    assert_eq!(env.context, Some(conv.to_string()));
    assert_eq!(env.published, common::at(3));
    assert_eq!(env.attributed_to.as_deref(), Some("ada@example.org"));

    let object = store.get(id).await.unwrap().unwrap();
    assert_eq!(object.index["encryption"], ["openpgp"]);
    assert_eq!(object.index["kind"], ["mail"]);
    assert_eq!(object.content["freesynergy.net:encryption"], "openpgp");
}

#[tokio::test]
async fn an_id_is_a_uuidv7_of_ours() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    assert_eq!(id.as_uuid().get_version_num(), 7);
}
