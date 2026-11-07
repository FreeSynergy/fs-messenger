//! The send flow after the check: echoes, downgrades, failures, replies and scheduling.

mod common;

use common::{alias, conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    DeliveryState, Encryption, Error, ExternalId, Ingest, Ingested, Messenger,
};
use fs_objects::{Query, Store};
use std::sync::Arc;

async fn notes(store: &Arc<dyn Store>) -> usize {
    store.count(&Query::new().of_type("Note")).await.unwrap()
}

#[tokio::test]
async fn the_echo_of_a_sent_message_is_recognised() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().conversation = Some(ExternalId::new("thread", "t"));
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("hi".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();

    let mut echo = raw("sent-1", "t", "hi");
    echo.sender = "me@example.org".into();
    assert_eq!(
        repo.ingest(me.id, echo).await.unwrap(),
        Ingested::Duplicate(id)
    );
    let answer = repo
        .ingest(me.id, raw("answer", "t", "thanks"))
        .await
        .unwrap();
    assert!(matches!(answer, Ingested::Stored(_)));
    assert_eq!(notes(&store).await, 2);
    assert_eq!(
        store
            .count(&Query::new().of_type("Collection"))
            .await
            .unwrap(),
        1,
        "same thread"
    );
}

#[tokio::test]
async fn a_weaker_answer_than_planned_is_reported_and_stored_as_it_is() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().encryption = Encryption::TransportOnly;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    let Err(Error::EncryptionDowngraded {
        message,
        encryption,
    }) = d.send(None).await
    else {
        panic!("a downgrade must be an error");
    };
    assert_eq!(encryption, Encryption::TransportOnly);
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, message).await.unwrap();
    assert_eq!(m.summary.encryption, Encryption::TransportOnly);
}

#[tokio::test]
async fn a_failed_send_is_visible_and_the_draft_stays() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    *provider.fail_send.lock().unwrap() = true;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    assert!(matches!(d.send(None).await, Err(Error::Provider(_))));
    assert_eq!(repo.drafts().await.unwrap().len(), 1);
    let conv = conversation_of(&repo).await;
    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(page.items[0].state, DeliveryState::Failed);
}

#[tokio::test]
async fn a_reply_goes_to_the_sender_with_the_parents_alias() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@example.org"]).await;
    let Ingested::Stored(parent) = repo.ingest(me.id, raw("m1", "t", "q")).await.unwrap() else {
        panic!();
    };
    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();
    let sent = provider.sent.lock().unwrap()[0].clone();
    assert_eq!(sent.to(), ["ada@example.org"]);
    assert_eq!(sent.in_reply_to(), [alias("m1")]);
    assert_eq!(sent.conversation(), [ExternalId::new("thread", "t")]);
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, Some(parent.to_string()));
}

#[tokio::test]
async fn sending_later_is_explicitly_not_possible_yet() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    let later = chrono::Utc::now() + chrono::Duration::hours(1);
    let Err(Error::Unsupported(u)) = d.send(Some(later)).await else {
        panic!("expected not possible");
    };
    assert_eq!(u.capability(), "message.schedule");
    assert_eq!(provider.sent_count(), 0);
}
