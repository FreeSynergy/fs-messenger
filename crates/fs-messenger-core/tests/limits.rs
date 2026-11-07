//! N1, C24: what the limit counts. Provisional values.

mod common;

use std::sync::Arc;

use common::{ME, account, alias, conversation_of, message_id, raw};
use fs_messenger_core::{
    Error, Ingest, Ingested, Messenger, MessengerConfig, MessengerRepo, RawAttachment, RawMessage,
    RawToolCall, SearchFilter, ToolCall, ToolCallState,
};
use fs_objects::{Blob, Store, TursoStore};

async fn limited(limit: usize) -> MessengerRepo {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let mut config = MessengerConfig::new(ME);
    config.max_message_bytes = limit;
    MessengerRepo::open(store, config).await.unwrap()
}

type Change = Box<dyn Fn(&mut RawMessage)>;

fn file(name: &str, size: usize) -> RawAttachment {
    RawAttachment {
        name: name.into(),
        media_type: "application/x-a".into(),
        data: vec![0; size],
    }
}

fn blob(size: usize) -> Blob {
    let media_type = "message/rfc822".to_string();
    let bytes = vec![1; size];
    Blob { media_type, bytes }
}

#[tokio::test]
async fn n1_every_part_counts_towards_the_limit() {
    let big = "x".repeat(2000);
    let many: Vec<_> = (0..40).map(|n| message_id(&format!("{n}{big}"))).collect();
    let mutations: Vec<(&str, Change)> = vec![
        (
            "subject",
            Box::new(|m| m.subject = Some("x".repeat(70_000))),
        ),
        (
            "name",
            Box::new(|m| m.conversation_name = Some("x".repeat(70_000))),
        ),
        ("sender", Box::new(|m| m.sender = "x".repeat(70_000))),
        ("to", Box::new(|m| m.to = vec!["x".repeat(70_000)])),
        ("job", Box::new(|m| m.job_ref = "x".repeat(70_000))),
        (
            "tasks",
            Box::new(|m| m.task_refs = vec!["x".repeat(70_000)]),
        ),
        (
            "links",
            Box::new({
                let l = many.clone();
                move |m| m.links.clone_from(&l)
            }),
        ),
        (
            "references",
            Box::new({
                let l = many.clone();
                move |m| m.references.clone_from(&l)
            }),
        ),
        (
            "aliases",
            Box::new(move |m| {
                m.aliases = (0..40)
                    .map(|n| alias(&format!("{n}{}", "y".repeat(2000))))
                    .collect();
            }),
        ),
        (
            "file name",
            Box::new(|m| m.attachments = vec![file(&"x".repeat(70_000), 1)]),
        ),
        (
            "tool result",
            Box::new(|m| {
                m.tool_calls = vec![RawToolCall {
                    agent_ref: "t".into(),
                    call: ToolCall {
                        result: "x".repeat(70_000),
                        ..ToolCall::default()
                    },
                }];
            }),
        ),
    ];
    let repo = limited(64 * 1024).await;
    let me = account(&repo, "me@example.org").await;
    for (n, (name, change)) in mutations.iter().enumerate() {
        let mut m = raw(&format!("uid-{n}"), "t", "small");
        change(&mut m);
        let got = repo.ingest(me.id, m).await.unwrap();
        assert!(matches!(got, Ingested::TooLarge { .. }), "{name}: {got:?}");
    }
}

#[tokio::test]
async fn n1_decoded_parts_are_not_counted_on_top_of_their_original() {
    let repo = limited(1000).await;
    let me = account(&repo, "me@example.org").await;
    let mut mail = raw("uid-1", "t", "text");
    mail.original = Some(blob(800));
    mail.attachments.push(file("a", 600));
    let got = repo.ingest(me.id, mail).await.unwrap();
    assert!(matches!(got, Ingested::Stored(_)), "{got:?}");

    let mut over = raw("uid-2", "t", "text");
    over.original = Some(blob(1000));
    let got = repo.ingest(me.id, over).await.unwrap();
    assert!(
        matches!(got, Ingested::TooLarge { .. }),
        "the original counts: {got:?}"
    );

    let mut hidden = raw("uid-3", "t", "text");
    hidden.original = Some(blob(10));
    hidden.attachments.push(file("a", 2000));
    let got = repo.ingest(me.id, hidden).await.unwrap();
    assert!(
        matches!(got, Ingested::TooLarge { .. }),
        "a small original hides nothing: {got:?}"
    );
}

#[tokio::test]
async fn n1_the_number_of_files_and_of_links_is_capped() {
    let repo = limited(10 * 1024 * 1024).await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("uid-1", "t", "x");
    m.attachments = (0..257).map(|n| file(&format!("{n}"), 1)).collect();
    let got = repo.ingest(me.id, m).await.unwrap();
    assert!(matches!(got, Ingested::TooLarge { .. }), "{got:?}");

    let mut m = raw("uid-2", "t", "x");
    m.links = (0..500).map(|n| message_id(&format!("{n}@x"))).collect();
    let Ingested::Stored(id) = repo.ingest(me.id, m).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().links.len(), 64);
}

#[tokio::test]
async fn n1_a_tool_call_over_the_limit_is_refused_and_nothing_changes() {
    let repo = limited(1000).await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("uid-1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let call = |result: String| ToolCall {
        state: ToolCallState::Working,
        result,
        ..ToolCall::default()
    };
    repo.record_tool_call(id, "t", call("ok".into()))
        .await
        .unwrap();
    let got = repo.record_tool_call(id, "t", call("x".repeat(2000))).await;
    assert!(matches!(got, Err(Error::TooLarge { .. })), "{got:?}");
    let conv = conversation_of(&repo).await;
    assert_eq!(
        repo.message(conv, id).await.unwrap().tool_calls[0].result,
        "ok"
    );
}

#[tokio::test]
async fn search_stops_at_256_kib_and_the_message_stays_whole() {
    let repo = limited(10 * 1024 * 1024).await;
    let me = account(&repo, "me@example.org").await;
    let text = format!("nearstart {} farbeyond", "ä".repeat(150 * 1024));
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m", "t", &text)).await.unwrap() else {
        panic!();
    };
    let filter = SearchFilter::default();
    let hits = repo.search("nearstart", &filter, None).await.unwrap();
    assert_eq!(hits.items.len(), 1);
    assert_eq!(
        repo.search("farbeyond", &filter, None)
            .await
            .unwrap()
            .items
            .len(),
        0
    );
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().body_text, text);
}
