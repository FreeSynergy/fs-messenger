//! C4, C15: a value this version does not know, or a record it cannot read, fails nothing else.

mod common;

use common::{account, conversation_of, raw_at, setup};
use fs_messenger_core::{
    DeliveryState, Encryption, Ingest, Ingested, Kind, Messenger, MessengerRepo, SearchFilter,
    ToolCall, ToolCallOutcome, ToolCallState,
};
use fs_objects::ObjectId;
use serde_json::{from_value, json};

async fn three(repo: &MessengerRepo) -> (ObjectId, Vec<ObjectId>) {
    let me = account(repo, "me@example.org").await;
    let mut ids = vec![];
    for n in 0..3 {
        let m = raw_at(&format!("m{n}"), "t", n);
        let Ingested::Stored(id) = repo.ingest(me.id, m).await.unwrap() else {
            panic!();
        };
        ids.push(id);
    }
    (conversation_of(repo).await, ids)
}

#[test]
fn unknown_values_read_as_unknown_without_a_guess() {
    let call: ToolCall = from_value(json!({"state": "paused", "outcome": "maybe"})).unwrap();
    assert_eq!(call.state, ToolCallState::Unknown);
    assert_eq!(call.outcome, ToolCallOutcome::Unknown);
    assert_eq!(from_value::<Kind>(json!("fax")).unwrap(), Kind::Unknown);
    let state: DeliveryState = from_value(json!("queued")).unwrap();
    assert_eq!(state, DeliveryState::Unknown);
    assert_eq!(Kind::default(), Kind::Unknown);
    assert_eq!(DeliveryState::default(), DeliveryState::Unknown);
    assert_eq!(ToolCall::default().state, ToolCallState::Unknown);
}

#[tokio::test]
async fn c4_a_record_with_values_from_a_newer_program_still_reads() {
    let (repo, store) = setup().await;
    let (conv, ids) = three(&repo).await;
    let mut newer = store.get(ids[1]).await.unwrap().unwrap();
    newer.content = json!({
        "kind": "fax",
        "freesynergy.net:delivery": "queued",
        "freesynergy.net:encryption": "quantum",
        "toolCalls": [{ "id": "c", "state": "paused", "outcome": "maybe" }],
    });
    store.put(newer, "newer-version").await.unwrap();

    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.skipped.len(), 0);
    let m = repo.message(conv, ids[1]).await.unwrap();
    assert_eq!(m.summary.kind, Kind::Unknown);
    assert_eq!(m.summary.state, DeliveryState::Unknown);
    assert_eq!(m.summary.encryption, Encryption::Unknown);
    assert_eq!(m.tool_calls[0].state, ToolCallState::Unknown);
}

#[tokio::test]
async fn c4_an_unreadable_record_is_skipped_and_reported_and_the_rest_works() {
    let (repo, store) = setup().await;
    let (conv, ids) = three(&repo).await;
    let mut broken = store.get(ids[1]).await.unwrap().unwrap();
    broken.content = json!({ "unread": "not a boolean" });
    store.put(broken, "corruption").await.unwrap();

    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.skipped, [ids[1]]);
    let filter = SearchFilter::default();
    let found = repo.search("", &filter, None).await.unwrap();
    assert_eq!((found.items.len(), found.skipped.len()), (2, 1));
    assert!(
        repo.message(conv, ids[1]).await.is_err(),
        "asked for by id it says so"
    );

    repo.mark_read(conv, ids[2]).await.unwrap();
    assert!(repo.conversations(None, false, None).await.is_ok());
}

#[tokio::test]
async fn c4_unreadable_records_do_not_block_next_unread_or_the_newest_message() {
    let (repo, store) = setup().await;
    let (conv, ids) = three(&repo).await;
    for id in [ids[0], ids[2]] {
        let mut broken = store.get(id).await.unwrap().unwrap();
        broken.content = json!({ "toolCalls": "not a list" });
        store.put(broken, "corruption").await.unwrap();
    }
    assert_eq!(
        repo.next_unread(None).await.unwrap().unwrap().envelope.id,
        ids[1]
    );
    repo.record_usage(conv, fs_messenger_core::Usage::default())
        .await
        .unwrap();
    repo.pin(conv, true).await.unwrap();
    repo.mark_read(conv, ids[1]).await.unwrap();
    let last = repo.open(conv).await.unwrap().last_message.unwrap();
    assert_eq!(last.envelope.id, ids[1]);
}

#[tokio::test]
async fn c4_an_unreadable_conversation_and_draft_are_skipped() {
    let (repo, store) = setup().await;
    let (conv, _) = three(&repo).await;
    let mut broken = store.get(conv).await.unwrap().unwrap();
    broken.content = json!({ "pinned": "yes" });
    store.put(broken, "corruption").await.unwrap();
    let page = repo.conversations(None, false, None).await.unwrap();
    assert_eq!(page.items.len(), 0);
    assert_eq!(page.skipped, [conv]);

    let mut d = repo.compose(None).await.unwrap();
    d.set_text("keep".into(), "text/plain".into())
        .await
        .unwrap();
    d.save().await.unwrap();
    let mut other = repo.compose(None).await.unwrap();
    let id = other.save().await.unwrap();
    let mut bad = store.get(id).await.unwrap().unwrap();
    bad.content = json!({ "to": "not a list" });
    store.put(bad, "corruption").await.unwrap();
    assert_eq!(repo.drafts().await.unwrap().len(), 1);
}

#[tokio::test]
async fn c4_a_draft_from_an_older_program_reads() {
    let (repo, store) = setup().await;
    account(&repo, "me@example.org").await;
    let mut d = repo.compose(None).await.unwrap();
    let id = d.save().await.unwrap();
    let mut old = store.get(id).await.unwrap().unwrap();
    old.content = json!({ "text": "old" });
    store.put(old, "old-version").await.unwrap();
    assert_eq!(repo.drafts().await.unwrap()[0].id, id);
    assert!(repo.resume_draft(id).await.is_ok());
}

#[tokio::test]
async fn c15_a_vanished_newest_message_is_none_not_an_error() {
    let (repo, store) = setup().await;
    let (conv, ids) = three(&repo).await;
    store.delete(ids[2], "another-program").await.unwrap();
    let summary = repo.open(conv).await.unwrap();
    assert!(summary.last_message.is_none());
    assert!(repo.conversations(None, false, None).await.is_ok());
}
