//! N3, N6, N8, N12, and a cursor going in circles.

mod common;

use common::{account, alias, conversation_of, message_id, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, FetchBatch, Ingest, Ingested, Messenger, MessengerRepo, ProviderCursor,
    RawMessage, RawToolCall, ToolCall,
};
use fs_objects::{ObjectId, Query};
use serde_json::json;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn n3_an_unreadable_local_record_does_not_stop_a_sync() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &[]).await;
    let broken = stored(&repo, me.id, raw("known", "t", "x")).await;
    let mut child = raw("child", "t", "waits");
    child.in_reply_to = Some(alias("parent"));
    let child = stored(&repo, me.id, child).await;
    for id in [broken, child] {
        let mut o = store.get(id).await.unwrap().unwrap();
        o.content = json!({ "toolCalls": "not a list" });
        store.put(o, "corruption").await.unwrap();
    }
    let conv = conversation_of(&repo).await;
    let mut o = store.get(conv).await.unwrap().unwrap();
    o.content = json!({ "pinned": "yes" });
    store.put(o, "corruption").await.unwrap();

    let mut replay = raw("known", "t", "x");
    replay.links.push(message_id("new-link@x"));
    provider.batches.lock().unwrap().push_back(FetchBatch {
        messages: vec![
            replay,
            raw("parent", "t", "arrives"),
            raw("next", "t", "goes on"),
        ],
        next: Some(ProviderCursor("c1".into())),
        more: false,
    });
    let report = repo.sync(me.id).await.unwrap();
    assert_eq!(
        (report.stored, report.duplicates, report.rejected),
        (2, 1, 0)
    );
    assert_eq!(provider.since.lock().unwrap().len(), 1);
    let readable = repo.conversations(None, false, None).await.unwrap().items;
    assert_eq!(
        readable.len(),
        1,
        "the thread goes on in a new conversation"
    );
    assert_eq!(
        readable[0].last_message.as_ref().unwrap().preview,
        "goes on"
    );
}

#[tokio::test]
async fn n6_control_characters_are_refused_or_lose_the_claim() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    for bad in ["a\r\nBcc: x", "a\nb", "a\tb"] {
        let mut m = raw("uid-1", "t", "x");
        m.aliases = vec![alias(bad)];
        assert!(
            matches!(repo.ingest(me.id, m).await, Err(Error::Invalid(_))),
            "{bad:?}"
        );
    }
    let mut m = raw("uid-1", "t", "x");
    m.links = vec![message_id("evil\r\nBcc: x"), message_id("fine@x")];
    m.references = vec![message_id("evil\r\nBcc: x"), message_id("root@x")];
    m.in_reply_to = Some(message_id("evil\r\nBcc: x"));
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    let kept = repo.message(conv, id).await.unwrap();
    assert_eq!(kept.links, [message_id("fine@x")]);
    assert_eq!(kept.references, [message_id("root@x")]);
    assert_eq!(kept.pending_reply, None);
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 1);
}

#[tokio::test]
async fn n8_the_first_arrived_wins_as_parent() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut first = raw("uid-1", "t", "first");
    first.links.push(message_id("same@x"));
    let first = stored(&repo, me.id, first).await;
    let mut later = raw("uid-2", "t", "later");
    later.links.push(message_id("same@x"));
    stored(&repo, me.id, later).await;
    let mut reply = raw("uid-3", "t", "reply");
    reply.in_reply_to = Some(message_id("same@x"));
    let reply = stored(&repo, me.id, reply).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, reply).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, Some(first.to_string()));
}

#[tokio::test]
async fn n12_a_bad_tool_call_goes_and_the_message_stays() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let call = |r: &str| RawToolCall {
        agent_ref: r.into(),
        call: ToolCall::default(),
    };
    let mut m = raw("uid-1", "t", "x");
    m.tool_calls = vec![call(""), call("good")];
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().tool_calls.len(), 1);
}

#[tokio::test]
async fn a_cursor_going_in_circles_ends_the_sync() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &[]).await;
    let batch = |c: &str| FetchBatch {
        messages: vec![],
        next: Some(ProviderCursor(c.into())),
        more: true,
    };
    let batches = [batch("a"), batch("b"), batch("a"), batch("b"), batch("a")];
    provider.batches.lock().unwrap().extend(batches);
    repo.sync(me.id).await.unwrap();
    assert_eq!(
        provider.since.lock().unwrap().len(),
        3,
        "a, b, and a again ends it"
    );
}
