//! N4: what a large body costs. The numbers here are what `DESIGN.md` reports.

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::{Ingest, Ingested, MessageAction, Messenger, SearchFilter};
use fs_store::Query;

const MIB: usize = 1024 * 1024;

#[tokio::test]
async fn n4_a_flag_does_not_copy_a_large_body() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let text = format!("findme {}", "x".repeat(5 * MIB));
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m", "t", &text)).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let before = store.log_stats().await.unwrap().payload_bytes;

    for on in [true, false, true, false] {
        repo.act(conv, id, MessageAction::Flag(on)).await.unwrap();
    }

    let growth = store.log_stats().await.unwrap().payload_bytes - before;
    eprintln!("4 flags on a 5 MiB body: {growth} bytes of log, {before} before them");
    assert!(growth < 3 * MIB, "four flags cost {growth} bytes of log");

    let object = store.get(id).await.unwrap().unwrap();
    assert!(serde_json::to_string(&object.content).unwrap().len() < MIB);
    assert_eq!(
        repo.message(conv, id).await.unwrap().body_text,
        text,
        "still whole"
    );
    let hits = repo
        .search("findme", &SearchFilter::default(), None)
        .await
        .unwrap();
    assert_eq!(hits.items.len(), 1, "still searchable");
}

#[tokio::test]
async fn n4_a_small_body_stays_in_the_message_and_a_large_one_goes_with_it() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let documents = Query::new().of_type("Document");
    let Ingested::Stored(small) = repo.ingest(me.id, raw("s", "t", "small")).await.unwrap() else {
        panic!();
    };
    assert_eq!(store.count(&documents).await.unwrap(), 0);

    let text = "y".repeat(MIB);
    let Ingested::Stored(large) = repo.ingest(me.id, raw("l", "t", &text)).await.unwrap() else {
        panic!();
    };
    assert_eq!(store.count(&documents).await.unwrap(), 1);
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, small).await.unwrap().body_text, "small");

    repo.act(conv, large, MessageAction::Delete).await.unwrap();
    assert_eq!(store.count(&documents).await.unwrap(), 0);
}

#[tokio::test]
async fn n4_a_large_body_goes_out_whole_and_can_be_forwarded() {
    let (repo, store) = setup().await;
    let (_, provider) =
        common::with_provider(&repo, fs_messenger_core::Encryption::Mls, &["b@x"]).await;
    let text = "z".repeat(MIB);
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["b@x".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text(text.clone(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].text(), text);
    let object = store.get(id).await.unwrap().unwrap();
    assert!(serde_json::to_string(&object.content).unwrap().len() < MIB / 2);

    let conv = conversation_of(&repo).await;
    let forward = MessageAction::Forward(vec!["b@x".into()]);
    repo.act(conv, id, forward).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[1].text(), text);
}
