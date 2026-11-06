//! N6 on the way out, N10, N11, receipts.

mod common;

use common::{alias, conversation_of, message_id, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, ExternalId, Ingest, Ingested, Messenger, MessengerRepo, RawMessage,
};
use fs_store::{ObjectId, Query};
use serde_json::json;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

async fn send_new(repo: &MessengerRepo, text: &str) -> ObjectId {
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text(text.into(), "text/plain".into()).await.unwrap();
    d.send(None).await.unwrap()
}

#[tokio::test]
async fn n6_what_was_stored_before_the_rule_never_reaches_a_provider() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@example.org"]).await;
    let parent = stored(&repo, me.id, raw("uid-1", "t", "q")).await;
    let mut old = store.get(parent).await.unwrap().unwrap();
    old.content["links"] = json!([{ "scheme": "message-id", "id": "evil\r\nBcc: x" }]);
    old.content["references"] = json!([{ "scheme": "message-id", "id": "root\r\nBcc: x" }]);
    store.put(old, "old-version").await.unwrap();

    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    d.send(None).await.unwrap();
    let sent = provider.sent.lock().unwrap()[0].clone();
    assert_eq!(sent.in_reply_to(), [alias("uid-1")]);
    assert_eq!(sent.references().len(), 0);
}

#[tokio::test]
async fn receipt_aliases_are_validated_like_any_other() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    {
        let mut receipt = provider.receipt.lock().unwrap();
        receipt.aliases = vec![message_id("evil\r\nBcc: x"), message_id("good@x")];
        receipt.conversation = Some(ExternalId::new("thread", "a|b"));
    }
    let id = send_new(&repo, "x").await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.receipts, [message_id("good@x")]);
}

#[tokio::test]
async fn n10_only_a_conversation_this_send_created_gives_way() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let first = send_new(&repo, "no thread known").await;
    let own = conversation_of(&repo).await;
    stored(&repo, me.id, raw("uid-1", "elsewhere", "other thread")).await;

    provider.receipt.lock().unwrap().conversation = Some(ExternalId::new("thread", "elsewhere"));
    let mut d = repo.compose(Some(first)).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("a reply".into(), "text/plain".into())
        .await
        .unwrap();
    let reply = d.send(None).await.unwrap();

    let m = repo.message(own, reply).await.unwrap();
    assert_eq!(
        m.summary.envelope.context,
        Some(own.to_string()),
        "it stays in its thread"
    );
    let conversations = Query::new().of_type("Collection");
    assert_eq!(store.count(&conversations).await.unwrap(), 2);
}

#[tokio::test]
async fn n11_a_reply_that_came_first_finds_a_message_of_ours_by_its_receipt() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut early = raw("uid-1", "t", "answer to something unknown");
    early.in_reply_to = Some(message_id("mine@x"));
    let early = stored(&repo, me.id, early).await;
    provider.receipt.lock().unwrap().aliases = vec![message_id("mine@x")];
    let mine = send_new(&repo, "the question").await;
    let page = repo.conversations(None, false, None).await.unwrap().items;
    let conv = page
        .iter()
        .find(|c| c.name == "ada@example.org")
        .unwrap()
        .id;
    let m = repo.message(conv, early).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, Some(mine.to_string()));
    assert_eq!(m.pending_reply, None);
}
