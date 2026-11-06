//! C19, C21, C23: one thread is one conversation, and an account is who it says it is.

mod common;

use common::{account, conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    AccountSetup, Encryption, Error, ExternalId, Ingest, Ingested, Messenger, MessengerRepo,
    RawMessage,
};
use fs_store::{ObjectId, Query};

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn c19_a_new_message_in_a_known_thread_joins_that_conversation() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    stored(&repo, me.id, raw("uid-1", "t", "earlier")).await;
    provider.receipt.lock().unwrap().conversation = Some(ExternalId::new("thread", "t"));
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("again".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.send(None).await.unwrap();

    let conversations = Query::new().of_type("Collection");
    assert_eq!(store.count(&conversations).await.unwrap(), 1);
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().id(), id);
    assert_eq!(repo.messages(conv, None).await.unwrap().items.len(), 2);
}

#[tokio::test]
async fn c21_an_account_keeps_its_identity_across_case_and_removal() {
    let (repo, store) = setup().await;
    let me = account(&repo, "Me@Example.org").await;
    let same = repo.add_account(AccountSetup {
        address: " me@example.org".into(),
        kind: "mail".into(),
        secret: String::new(),
    });
    assert!(matches!(same.await, Err(Error::Conflict(_))));
    let id = stored(&repo, me.id, raw("uid-1", "t", "x")).await;

    repo.remove_account(me.id).await.unwrap();
    let back = account(&repo, "me@example.org").await;
    assert_ne!(back.id, me.id);
    let replay = repo.ingest(back.id, raw("uid-1", "t", "x")).await.unwrap();
    assert_eq!(replay, Ingested::Duplicate(id));
    stored(&repo, back.id, raw("uid-2", "t", "new in the old thread")).await;
    let conversations = Query::new().of_type("Collection");
    assert_eq!(store.count(&conversations).await.unwrap(), 1);

    let (provider, _) = (
        common::TestProvider::new(Encryption::Mls, &["ada@example.org"]),
        0,
    );
    repo.register_provider(back.id, provider.clone());
    let mut d = repo.compose(Some(id)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    d.send(None).await.unwrap();
    assert_eq!(
        provider.sent_count(),
        1,
        "a reply goes out over the account as it is now"
    );
}

#[tokio::test]
async fn c23_a_stranger_cannot_claim_a_mail_is_mine() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut forged = raw("uid-1", "t", "x");
    forged.outgoing = true;
    let id = stored(&repo, me.id, forged).await;
    let mut mine = raw("uid-2", "t", "y");
    mine.outgoing = true;
    mine.sender = "ME@example.org".into();
    let own = stored(&repo, me.id, mine).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert!(!m.outgoing && m.summary.unread);
    let m = repo.message(conv, own).await.unwrap();
    assert!(m.outgoing && !m.summary.unread);
}
