//! C5, C20: who can make a message count as known, and what survives.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{account, alias, conversation_of, message_id, raw, setup, with_provider};
use fs_messenger_core::{
    AccountSetup, Encryption, EncryptionPlan, Error, ExternalId, Ingest, Ingested, MessageAction,
    Messenger, MessengerRepo, Outgoing, Provider, RawMessage, Result, SendReceipt,
};
use fs_store::{ObjectId, Query};

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn c5_a_forged_message_id_does_not_suppress_the_real_mail() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut forged = raw("uid-1", "t", "the forgery came first");
    forged.aliases.push(message_id("real@bank.example"));
    let first = stored(&repo, me.id, forged).await;

    let mut real = raw("uid-2", "t", "the real mail");
    real.aliases.push(message_id("real@bank.example"));
    let second = stored(&repo, me.id, real).await;

    assert_ne!(first, second);
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 2);
    let conv = conversation_of(&repo).await;
    let kept = repo.message(conv, first).await.unwrap();
    assert_eq!(kept.aliases, [alias("uid-1")], "a Message-ID never dedupes");
    assert_eq!(kept.links, [message_id("real@bank.example")]);
}

#[tokio::test]
async fn c5_the_same_mail_in_two_folders_shows_twice_rather_than_getting_lost() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    for uid in ["inbox/1", "archive/7"] {
        let mut m = raw(uid, "t", "same mail");
        m.links.push(message_id("x@example.org"));
        stored(&repo, me.id, m).await;
    }
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 2);
}

#[tokio::test]
async fn c5_a_provider_issued_id_still_recognises_a_replay() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let id = stored(&repo, me.id, raw("uid-1", "t", "x")).await;
    let again = repo.ingest(me.id, raw("uid-1", "t", "x")).await.unwrap();
    assert_eq!(again, Ingested::Duplicate(id));
}

#[tokio::test]
async fn c5_only_a_sender_chosen_id_is_no_id_at_all() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("x", "t", "x");
    m.aliases = vec![message_id("x@example.org")];
    assert!(matches!(
        repo.ingest(me.id, m).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn c5_a_returning_copy_of_ours_matches_only_from_our_own_address() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().aliases = vec![message_id("mine@example.org")];
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("hi".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();

    let mut stranger = raw("uid-5", "t", "claims to be my mail");
    stranger.links.push(message_id("mine@example.org"));
    stored(&repo, me.id, stranger).await;

    let mut copy = raw("uid-6", "t", "hi");
    copy.sender = "ME@example.org ".into();
    copy.links.push(message_id("mine@example.org"));
    assert_eq!(
        repo.ingest(me.id, copy.clone()).await.unwrap(),
        Ingested::Duplicate(id)
    );

    let mut by_uid = raw("uid-6", "t", "hi");
    by_uid.sender = "someone@else.example".into();
    assert_eq!(
        repo.ingest(me.id, by_uid).await.unwrap(),
        Ingested::Duplicate(id),
        "uid learned"
    );
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 2);
}

#[tokio::test]
async fn c5_a_link_threads_a_reply_and_reaches_the_answer_on_the_way_out() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@example.org"]).await;
    let mut parent = raw("uid-1", "t", "question");
    parent.links.push(message_id("q@example.org"));
    let parent = stored(&repo, me.id, parent).await;
    let mut reply = raw("uid-2", "t", "more");
    reply.in_reply_to = Some(message_id("q@example.org"));
    let reply = stored(&repo, me.id, reply).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, reply).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, Some(parent.to_string()));

    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("answer".into(), "text/plain".into())
        .await
        .unwrap();
    d.send(None).await.unwrap();
    let sent = provider.sent.lock().unwrap()[0].clone();
    assert!(sent.in_reply_to().contains(&message_id("q@example.org")));
}

/// Deletes the message in the moment it goes out.
struct DeletesWhileSending {
    repo: MessengerRepo,
    conversation: ObjectId,
}

#[async_trait]
impl Provider for DeletesWhileSending {
    async fn encryption_plan(&self, _: &[String]) -> Result<EncryptionPlan> {
        Ok(EncryptionPlan {
            encryption: Encryption::Mls,
            missing_keys: vec![],
        })
    }
    async fn send(&self, m: &Outgoing) -> Result<SendReceipt> {
        self.repo
            .act(self.conversation, m.id(), MessageAction::Delete)
            .await?;
        Ok(SendReceipt {
            aliases: vec![message_id("gone@example.org")],
            conversation: None,
            encryption: m.encryption(),
        })
    }
}

#[tokio::test]
async fn c20_a_message_deleted_on_its_way_out_does_not_come_back_as_its_copy() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let parent = stored(&repo, me.id, raw("uid-1", "t", "q")).await;
    let conversation = conversation_of(&repo).await;
    repo.register_provider(
        me.id,
        Arc::new(DeletesWhileSending {
            repo: repo.clone(),
            conversation,
        }),
    );
    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();

    let mut copy = raw("uid-9", "t", "a");
    copy.sender = "me@example.org".into();
    copy.links.push(message_id("gone@example.org"));
    assert_eq!(
        repo.ingest(me.id, copy).await.unwrap(),
        Ingested::Duplicate(id)
    );
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 1);
}
