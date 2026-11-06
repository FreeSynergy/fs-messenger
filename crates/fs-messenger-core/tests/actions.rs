//! `act`: edit keeps the id, delete leaves a tombstone, reactions, flags, and what is refused.

mod common;

use common::{conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo, RawAttachment,
    ToolCallDecision,
};
use fs_store::{Blob, ObjectId, Query};

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
async fn an_edit_keeps_the_id_and_replaces_the_text_in_every_view() {
    let (repo, store) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (conv, id) = sent_message(&repo).await;
    let published = repo
        .message(conv, id)
        .await
        .unwrap()
        .summary
        .envelope
        .published;

    repo.act(conv, id, MessageAction::Edit("second text".into()))
        .await
        .unwrap();

    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.id(), id);
    assert_eq!(m.body_text, "second text");
    assert_eq!(m.summary.preview, "second text");
    assert_eq!(m.summary.envelope.published, published);
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 1);

    let hits = repo
        .search("second", &Default::default(), None)
        .await
        .unwrap();
    assert_eq!(hits.items.len(), 1);
    assert!(
        repo.search("first", &Default::default(), None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn only_my_own_message_can_be_edited_and_the_original_never_changes() {
    let (repo, _) = setup().await;
    let (me, _) = with_provider(&repo, Encryption::Mls, &[]).await;
    let mut incoming = raw("m1", "t", "their words");
    incoming.original = Some(Blob {
        media_type: "message/rfc822".into(),
        bytes: b"raw".to_vec(),
    });
    let Ingested::Stored(id) = repo.ingest(me.id, incoming).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let refused = repo
        .act(conv, id, MessageAction::Edit("my words".into()))
        .await;
    assert!(matches!(refused, Err(Error::Conflict(_))));
    repo.act(conv, id, MessageAction::Flag(true)).await.unwrap();
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.body_text, "their words");
    assert_eq!(m.original.unwrap().bytes, b"raw");
    assert!(m.summary.flagged);
}

#[tokio::test]
async fn an_empty_edit_is_refused() {
    let (repo, _) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (conv, id) = sent_message(&repo).await;
    let empty = repo.act(conv, id, MessageAction::Edit("  ".into())).await;
    assert!(matches!(empty, Err(Error::Invalid(_))));
    assert_eq!(
        repo.message(conv, id).await.unwrap().body_text,
        "first text"
    );
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
async fn a_reaction_counts_once_can_change_and_can_be_taken_back() {
    let (repo, _) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (conv, id) = sent_message(&repo).await;
    let reactions =
        |repo: MessengerRepo| async move { repo.message(conv, id).await.unwrap().reactions };
    for _ in 0..2 {
        repo.act(conv, id, MessageAction::React("+1".into()))
            .await
            .unwrap();
    }
    let r = reactions(repo.clone()).await;
    assert_eq!(
        (r.len(), r[0].key.as_str(), r[0].count, r[0].mine),
        (1, "+1", 1, true)
    );
    repo.act(conv, id, MessageAction::React("heart".into()))
        .await
        .unwrap();
    let r = reactions(repo.clone()).await;
    assert_eq!((r.len(), r[0].key.as_str()), (1, "heart"));
    repo.act(conv, id, MessageAction::React(String::new()))
        .await
        .unwrap();
    assert!(reactions(repo.clone()).await.is_empty());
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
    assert!(repo.messages(conv, None).await.unwrap().items.is_empty());
    assert!(matches!(
        repo.attachment(blob).await,
        Err(Error::NotFound(_))
    ));
    let tombstone = store.get(id).await.unwrap().unwrap();
    assert_eq!(tombstone.envelope.object_type, "Tombstone");
    assert_eq!(tombstone.content, serde_json::json!({}));
    assert!(tombstone.original.is_none());
}
