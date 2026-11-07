//! The change feed, from real store activities to the contract's events.

mod common;

use std::sync::Arc;

use common::{
    ME, account, collector, conversation_of, next_event, no_event, raw, raw_at, setup,
    with_provider,
};
use fs_messenger_core::{
    DeliveryState, Encryption, Event, EventFilter, Ingest, Ingested, Kind, MessageAction,
    Messenger, MessengerConfig, MessengerRepo, ToolCall, ToolCallState,
};
use fs_objects::{OpenOptions, TursoStore};

#[tokio::test]
async fn created_updated_deleted_arrive_in_order() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let (l, mut rx) = collector();
    let _sub = repo.subscribe(EventFilter::default(), l).await.unwrap();

    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let Event::Created(s) = next_event(&mut rx).await else {
        panic!("expected created");
    };
    assert_eq!(s.envelope.id, id);

    let conv = conversation_of(&repo).await;
    repo.act(conv, id, MessageAction::Flag(true)).await.unwrap();
    let Event::Updated(s) = next_event(&mut rx).await else {
        panic!("expected updated");
    };
    assert!(s.flagged);

    repo.act(conv, id, MessageAction::Delete).await.unwrap();
    assert_eq!(next_event(&mut rx).await, Event::Deleted(id));
    no_event(&mut rx).await;
}

#[tokio::test]
async fn sending_tells_the_state_and_a_replay_or_alias_is_silent() {
    let (repo, _) = setup().await;
    let (me, _) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let (l, mut rx) = collector();
    let _sub = repo.subscribe(EventFilter::default(), l).await.unwrap();

    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();

    let Event::Created(s) = next_event(&mut rx).await else {
        panic!("expected created");
    };
    assert_eq!(s.state, DeliveryState::Sending);
    let Event::State(c) = next_event(&mut rx).await else {
        panic!("expected state");
    };
    assert_eq!((c.message, c.state), (id, DeliveryState::Sent));
    no_event(&mut rx).await;

    let mut with_new_alias = raw("sent-1", "t", "x");
    with_new_alias.sender = "me@example.org".into();
    with_new_alias.aliases.push(common::alias("uid-5"));
    repo.ingest(me.id, with_new_alias).await.unwrap();
    no_event(&mut rx).await;
}

#[tokio::test]
async fn a_tool_call_change_is_one_event_with_the_conversation() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let (l, mut rx) = collector();
    let _sub = repo.subscribe(EventFilter::default(), l).await.unwrap();
    let call = ToolCall {
        state: ToolCallState::Working,
        summary: "s".into(),
        ..ToolCall::default()
    };
    let stored = repo.record_tool_call(id, "t1", call).await.unwrap();
    let Event::ToolCall(c) = next_event(&mut rx).await else {
        panic!("expected a tool call change");
    };
    assert_eq!((c.message, c.conversation), (id, conv));
    assert_eq!(c.call, stored);
    no_event(&mut rx).await;
}

#[tokio::test]
async fn filters_and_dropping_end_what_is_heard() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    repo.ingest(me.id, raw("seed", "a", "x")).await.unwrap();
    let a = conversation_of(&repo).await;

    let (l1, mut only_a) = collector();
    let filter = EventFilter {
        conversations: vec![a],
        kinds: vec![],
    };
    let _s1 = repo.subscribe(filter, l1).await.unwrap();
    let (l2, mut only_chat) = collector();
    let filter = EventFilter {
        conversations: vec![],
        kinds: vec![Kind::Chat],
    };
    let _s2 = repo.subscribe(filter, l2).await.unwrap();
    let (l3, mut dropped) = collector();
    let s3 = repo.subscribe(EventFilter::default(), l3).await.unwrap();
    drop(s3);

    let mut chat = raw("c", "b", "chat");
    chat.kind = Kind::Chat;
    repo.ingest(me.id, chat).await.unwrap();
    repo.ingest(me.id, raw("m", "a", "mail")).await.unwrap();

    let Event::Created(s) = next_event(&mut only_a).await else {
        panic!()
    };
    assert_eq!(s.preview, "mail");
    no_event(&mut only_a).await;
    let Event::Created(s) = next_event(&mut only_chat).await else {
        panic!()
    };
    assert_eq!(s.preview, "chat");
    no_event(&mut only_chat).await;
    no_event(&mut dropped).await;
    assert!(
        dropped.recv().await.is_none(),
        "the dropped subscription released its listener"
    );
}

#[tokio::test]
async fn a_slow_listener_misses_nothing() {
    let options = OpenOptions {
        allow_sensitive_unencrypted: true,
        subscribe_capacity: 2,
        ..OpenOptions::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let store = TursoStore::open_with(dir.path().join("lag.db"), options)
        .await
        .unwrap();
    let repo = MessengerRepo::open(Arc::new(store), MessengerConfig::new(ME))
        .await
        .unwrap();
    let me = account(&repo, "me@example.org").await;

    let (l, mut rx) = collector();
    let _sub = repo.subscribe(EventFilter::default(), l).await.unwrap();
    for n in 0..30 {
        repo.ingest(me.id, raw_at(&format!("m{n}"), "t", n))
            .await
            .unwrap();
    }
    let mut seen = vec![];
    while seen.len() < 30 {
        if let Event::Created(s) = next_event(&mut rx).await {
            seen.push(s.preview);
        }
    }
    let expected: Vec<_> = (0..30).map(|n| format!("m{n}")).collect();
    assert_eq!(seen, expected);
}
