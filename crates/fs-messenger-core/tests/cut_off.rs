//! A connector that only holds a cut-off start of a message, and a body object already gone.

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::{
    Ingest, Ingested, MessageAction, Messenger, MessengerRepo, RawAttachment, RawMessage,
};
use fs_objects::{Blob, ObjectId, Query};

const REAL: u64 = 99 * 1024 * 1024;

fn cut_off(id: &str) -> RawMessage {
    let mut m = raw(id, "t", "the start of it");
    m.too_large = Some(REAL);
    m.original = Some(Blob {
        media_type: "message/rfc822".into(),
        bytes: b"From: ada\r\n\r\nthe start of".to_vec(),
    });
    m.attachments.push(RawAttachment {
        name: "half.png".into(),
        media_type: "image/png".into(),
        data: vec![1; 10],
    });
    m
}

async fn stubbed(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> (ObjectId, usize) {
    let got = repo.ingest(me, m).await.unwrap();
    let Ingested::TooLarge { id, size, .. } = got else {
        panic!("expected a stub: {got:?}");
    };
    (id, size)
}

#[tokio::test]
async fn a_cut_off_message_is_a_stub_under_any_limit_and_its_bytes_are_no_original() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let (id, size) = stubbed(&repo, me.id, cut_off("uid-1")).await;
    assert_eq!(size as u64, REAL);

    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.too_large, REAL);
    assert_eq!(m.original, None);
    assert_eq!(m.body_text, "");
    assert_eq!(m.attachments.len(), 0);
    assert_eq!(store.get(id).await.unwrap().unwrap().original, None);
    let documents = Query::new().of_type("Document");
    assert_eq!(
        store.count(&documents).await.unwrap(),
        0,
        "no half file either"
    );
}

#[tokio::test]
async fn a_claim_below_what_arrived_or_of_zero_is_still_a_stub_with_a_size() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut low = cut_off("uid-1");
    low.too_large = Some(0);
    let (id, size) = stubbed(&repo, me.id, low).await;
    assert!(size > 0);
    let conv = conversation_of(&repo).await;
    assert_eq!(
        repo.message(conv, id).await.unwrap().summary.too_large,
        size as u64
    );

    let mut empty = raw("uid-2", "t", "");
    empty.sender = String::new();
    empty.too_large = Some(0);
    let (_, size) = stubbed(&repo, me.id, empty).await;
    assert!(size > 0, "0 would say it is not too large");
}

#[tokio::test]
async fn unset_keeps_the_message_whole() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut whole = cut_off("uid-1");
    whole.too_large = None;
    let Ingested::Stored(id) = repo.ingest(me.id, whole).await.unwrap() else {
        panic!("expected a whole message");
    };
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.too_large, 0);
    assert_eq!(m.body_text, "the start of it");
    assert_ne!(m.original, None);
}

#[tokio::test]
async fn a_cut_off_replay_does_not_replace_an_undecrypted_message() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut locked = raw("uid-1", "t", "ciphertext");
    locked.undecrypted = true;
    let Ingested::Stored(id) = repo.ingest(me.id, locked).await.unwrap() else {
        panic!();
    };
    let got = repo.ingest(me.id, cut_off("uid-1")).await.unwrap();
    assert_eq!(got, Ingested::Duplicate(id));
    let conv = conversation_of(&repo).await;
    assert!(repo.message(conv, id).await.unwrap().summary.undecrypted);
}

#[tokio::test]
async fn deleting_a_message_whose_body_object_is_gone_works() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let text = "y".repeat(1024 * 1024);
    let Ingested::Stored(id) = repo.ingest(me.id, raw("uid-1", "t", &text)).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let body = store
        .query(&Query::new().of_type("Document"))
        .await
        .unwrap();
    store.delete(body[0].envelope.id, "damage").await.unwrap();

    repo.act(conv, id, MessageAction::Delete).await.unwrap();

    assert_eq!(repo.open(conv).await.unwrap().unread_count, 0);
    assert_eq!(repo.messages(conv, None).await.unwrap().items.len(), 0);
}
