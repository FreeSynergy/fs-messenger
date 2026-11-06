//! Review findings on the send path: one provider per send, drafts that end, nothing half stored.

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use common::{account, conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, EncryptionPlan, Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo,
    Outgoing, Provider, Result, SendReceipt,
};
use fs_store::{ObjectId, Query};

/// Plans well, and in that moment another provider gets registered for the account.
struct SwapsWhilePlanning {
    repo: MessengerRepo,
    account: ObjectId,
    other: Arc<Thief>,
    sent: Mutex<usize>,
}

#[derive(Default)]
struct Thief {
    got: Mutex<usize>,
}

#[async_trait]
impl Provider for Thief {
    async fn send(&self, _: &Outgoing) -> Result<SendReceipt> {
        *self.got.lock().unwrap() += 1;
        Ok(SendReceipt::default())
    }
}

#[async_trait]
impl Provider for SwapsWhilePlanning {
    async fn encryption_plan(&self, _: &[String]) -> Result<EncryptionPlan> {
        self.repo
            .register_provider(self.account, self.other.clone());
        Ok(EncryptionPlan {
            encryption: Encryption::Mls,
            missing_keys: vec![],
        })
    }
    async fn send(&self, m: &Outgoing) -> Result<SendReceipt> {
        *self.sent.lock().unwrap() += 1;
        Ok(SendReceipt {
            encryption: m.encryption(),
            ..SendReceipt::default()
        })
    }
}

#[tokio::test]
async fn c1_whoever_made_the_plan_gets_the_message() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let thief = Arc::new(Thief::default());
    let planner = Arc::new(SwapsWhilePlanning {
        repo: repo.clone(),
        account: me.id,
        other: thief.clone(),
        sent: Mutex::new(0),
    });
    repo.register_provider(me.id, planner.clone());
    // `set_recipients` plans too and swaps early, so the planner goes back in before the send.
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("secret".into(), "text/plain".into())
        .await
        .unwrap();
    repo.register_provider(me.id, planner.clone());
    d.send(None).await.unwrap();
    assert_eq!(*thief.got.lock().unwrap(), 0, "the plaintext stays away");
    assert_eq!(*planner.sent.lock().unwrap(), 1);
}

#[tokio::test]
async fn c9_a_downgraded_draft_is_over_and_a_retry_cannot_send_again() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().encryption = Encryption::TransportOnly;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    assert!(matches!(
        d.send(None).await,
        Err(Error::EncryptionDowngraded { .. })
    ));
    assert!(matches!(d.send(None).await, Err(Error::Conflict(_))));
    assert_eq!(provider.sent_count(), 1);
}

#[tokio::test]
async fn c10_edit_and_react_say_not_possible_and_change_nothing() {
    let (repo, store) = setup().await;
    with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("mine".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.send(None).await.unwrap();
    let conv = conversation_of(&repo).await;
    let before = store.get(id).await.unwrap().unwrap();
    for (action, capability) in [
        (MessageAction::Edit("new".into()), "message.edit"),
        (MessageAction::React("+1".into()), "message.react"),
        (MessageAction::React(String::new()), "message.react"),
    ] {
        let Err(Error::Unsupported(u)) = repo.act(conv, id, action).await else {
            panic!("{capability} must say not possible");
        };
        assert_eq!(u.capability(), capability);
        assert!(!repo.capabilities().is_supported(capability));
    }
    assert_eq!(store.get(id).await.unwrap().unwrap(), before);
}

#[tokio::test]
async fn c16_answering_a_message_deleted_meanwhile_still_works() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@example.org"]).await;
    let Ingested::Stored(parent) = repo.ingest(me.id, raw("m1", "t", "q")).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    repo.act(conv, parent, MessageAction::Delete).await.unwrap();
    d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].in_reply_to().len(), 0);
}

#[tokio::test]
async fn c17_a_missing_file_stops_the_send_before_anything_is_stored() {
    let (repo, store) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.attach("a".into(), "text/plain".into(), vec![1])
        .await
        .unwrap();
    let file = store
        .query(&Query::new().of_type("Document"))
        .await
        .unwrap();
    store.delete(file[0].envelope.id, "test").await.unwrap();
    assert!(d.send(None).await.is_err());
    assert_eq!(provider.sent_count(), 0);
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 0);
    let conversations = Query::new().of_type("Collection");
    assert_eq!(store.count(&conversations).await.unwrap(), 0);
}

#[tokio::test]
async fn c18_a_reply_keeps_the_account_of_its_conversation() {
    let (repo, _) = setup().await;
    let a = account(&repo, "a@example.org").await;
    let b = account(&repo, "b@example.org").await;
    let Ingested::Stored(parent) = repo.ingest(a.id, raw("m1", "t", "q")).await.unwrap() else {
        panic!();
    };
    let mut d = repo.compose(Some(parent)).await.unwrap();
    assert!(matches!(d.set_account(b.id).await, Err(Error::Conflict(_))));
    let mut fresh = repo.compose(None).await.unwrap();
    fresh.set_account(b.id).await.unwrap();
}

#[tokio::test]
async fn hidden_recipients_never_appear_in_a_conversation_name() {
    let (repo, _) = setup().await;
    with_provider(
        &repo,
        Encryption::Mls,
        &["bob@example.org", "hidden@example.org"],
    )
    .await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(
        vec!["bob@example.org".into()],
        vec![],
        vec!["hidden@example.org".into()],
    )
    .await
    .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    d.send(None).await.unwrap();
    let list = repo.conversations(None, false, None).await.unwrap().items;
    assert_eq!(list[0].name, "bob@example.org");
}
