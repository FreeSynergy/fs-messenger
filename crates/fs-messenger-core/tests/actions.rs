//! `act`: delete leaves a tombstone, flags, and what is refused.

mod common;

use common::{conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo, RawAttachment,
    ToolCallDecision,
};
use fs_store::{Blob, ObjectId};

async fn sent_message(repo: &MessengerRepo) -> (ObjectId, ObjectId) {
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("first text".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.send(None).await.unwrap();
    (conversation_of(repo).await, id)
}

#[tokio::test]
async fn the_unset_action_does_nothing_and_is_refused() {
    let (repo, store) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (conv, id) = sent_message(&repo).await;
    let before = store.get(id).await.unwrap().unwrap();
    assert!(matches!(
        repo.act(conv, id, MessageAction::None).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.get(id).await.unwrap().unwrap(), before);
}

#[tokio::test]
async fn decide_and_stop_are_explicitly_not_possible_yet() {
    let (repo, store) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (conv, id) = sent_message(&repo).await;
    let before = store.get(id).await.unwrap().unwrap();
    let decide = MessageAction::Decide(ToolCallDecision::default());
    let Err(Error::Unsupported(u)) = repo.act(conv, id, decide).await else {
        panic!("expected not possible");
    };
    assert_eq!(u.capability(), "toolcall.decide");
    assert_eq!(store.get(id).await.unwrap().unwrap(), before);
    let Err(Error::Unsupported(u)) = repo.stop(conv).await else {
        panic!("expected not possible");
    };
    assert_eq!(u.capability(), "turn.stop");
}

#[tokio::test]
async fn delete_removes_content_and_attachment_keeps_counts_true() {
    let (repo, store) = setup().await;
    let (me, _) = with_provider(&repo, Encryption::Mls, &[]).await;
    let mut m = raw("m1", "t", "bye");
    m.attachments.push(RawAttachment {
        name: "a.bin".into(),
        media_type: "application/x-a".into(),
        data: vec![1, 2, 3],
    });
    let Ingested::Stored(id) = repo.ingest(me.id, m).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let got = repo.message(conv, id).await.unwrap();
    let blob = got.attachments[0].blob;
    assert_eq!(got.attachments[0].size, 3);
    assert_eq!(repo.attachment(blob).await.unwrap().bytes, [1, 2, 3]);
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 1);

    repo.act(conv, id, MessageAction::Delete).await.unwrap();

    let summary = repo.open(conv).await.unwrap();
    assert_eq!(summary.unread_count, 0);
    assert!(summary.last_message.is_none());
    assert_eq!(repo.messages(conv, None).await.unwrap().items.len(), 0);
    assert!(matches!(
        repo.attachment(blob).await,
        Err(Error::NotFound(_))
    ));
    let tombstone = store.get(id).await.unwrap().unwrap();
    assert_eq!(tombstone.envelope.object_type, "Tombstone");
    assert_eq!(tombstone.content, serde_json::json!({}));
    assert!(tombstone.original.is_none());
}

#[tokio::test]
async fn a_flag_stays_local_and_changes_only_the_flag() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &[]).await;
    let mut incoming = raw("m1", "t", "their words");
    incoming.original = Some(Blob {
        media_type: "message/rfc822".into(),
        bytes: b"raw".to_vec(),
    });
    let Ingested::Stored(id) = repo.ingest(me.id, incoming).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    repo.act(conv, id, MessageAction::Flag(true)).await.unwrap();
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.body_text, "their words");
    assert_eq!(m.original.unwrap().bytes, b"raw");
    assert!(m.summary.flagged);
    assert_eq!(provider.sent_count(), 0);
}
