//! M1, M2, M5, M6, K4, K5: names and headers that stay small, a missing body, stubs.

mod common;

use common::{account, alias, conversation_of, message_id, raw, setup, with_provider};
use fs_messenger_core::{
    CONTRACT_VERSION, Encryption, Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo,
    RawMessage,
};
use fs_objects::{ObjectId, Query};

const MIB: usize = 1024 * 1024;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn m1_a_long_subject_does_not_become_a_long_conversation_name() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("uid-1", "t", "x");
    m.subject = Some("s".repeat(1000));
    stored(&repo, me.id, m).await;
    let list = repo.conversations(None, false, None).await.unwrap().items;
    assert_eq!(list[0].name.chars().count(), 200);
}

#[tokio::test]
async fn m1_many_recipients_do_not_become_a_long_conversation_name() {
    let (repo, _) = setup().await;
    let everyone: Vec<String> = (0..100).map(|n| format!("person{n}@example.org")).collect();
    let keys: Vec<&str> = everyone.iter().map(String::as_str).collect();
    with_provider(&repo, Encryption::Mls, &keys).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(everyone.clone(), vec![], vec![])
        .await
        .unwrap();
    d.send(None).await.unwrap();
    let list = repo.conversations(None, false, None).await.unwrap().items;
    assert_eq!(list[0].name.chars().count(), 200);
}

#[tokio::test]
async fn m1_a_5_mib_subject_or_to_gives_a_stub_and_costs_no_log_on_a_flag() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut big_subject = raw("uid-1", "t1", "x");
    big_subject.subject = Some("s".repeat(5 * MIB));
    let got = repo.ingest(me.id, big_subject).await.unwrap();
    assert!(matches!(got, Ingested::TooLarge { .. }), "{got:?}");

    let mut big_to = raw("uid-2", "t2", "x");
    big_to.to = vec!["t".repeat(5 * MIB)];
    let Ingested::TooLarge { id, .. } = repo.ingest(me.id, big_to).await.unwrap() else {
        panic!("a 5 MiB `to` is too large");
    };
    let conv = conversation_of(&repo).await;
    let before = store.log_stats().await.unwrap().payload_bytes;
    for on in [true, false, true, false] {
        repo.act(conv, id, MessageAction::Flag(on)).await.unwrap();
    }
    let growth = store.log_stats().await.unwrap().payload_bytes - before;
    assert!(growth < 100 * 1024, "four flags cost {growth} bytes of log");
    let list = repo.conversations(None, false, None).await.unwrap().items;
    assert!(list.iter().all(|c| c.name.len() <= 200));
}

#[tokio::test]
async fn m1_headers_up_to_64_kib_are_taken_and_tool_results_are_not_headers() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut fits = raw("uid-1", "t", "x");
    fits.to = vec!["t".repeat(60 * 1024)];
    assert!(matches!(
        repo.ingest(me.id, fits).await.unwrap(),
        Ingested::Stored(_)
    ));
    let mut over = raw("uid-2", "t", "x");
    over.to = vec!["t".repeat(65 * 1024)];
    let got = repo.ingest(me.id, over).await.unwrap();
    assert!(matches!(got, Ingested::TooLarge { .. }), "{got:?}");
    let mut tool = raw("uid-3", "t", "x");
    tool.tool_calls.push(fs_messenger_core::RawToolCall {
        agent_ref: "t".into(),
        call: fs_messenger_core::ToolCall {
            result: "r".repeat(MIB),
            ..fs_messenger_core::ToolCall::default()
        },
    });
    assert!(matches!(
        repo.ingest(me.id, tool).await.unwrap(),
        Ingested::Stored(_)
    ));
}

#[tokio::test]
async fn m2_a_missing_body_is_named_and_forward_sends_nothing() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["b@x"]).await;
    let id = stored(&repo, me.id, raw("uid-1", "t", &"y".repeat(MIB))).await;
    let conv = conversation_of(&repo).await;
    let body = store
        .query(&Query::new().of_type("Document"))
        .await
        .unwrap();
    store.delete(body[0].envelope.id, "damage").await.unwrap();

    let read = repo.message(conv, id).await;
    assert!(matches!(read, Err(Error::BodyMissing { message, .. }) if message == id));
    let forward = MessageAction::Forward(vec!["b@x".into()]);
    let answer = repo.act(conv, id, forward).await;
    assert!(
        matches!(answer, Err(Error::BodyMissing { .. })),
        "{answer:?}"
    );
    assert_eq!(provider.sent_count(), 0);
    assert_eq!(
        repo.messages(conv, None).await.unwrap().items.len(),
        1,
        "it still lists"
    );
}

#[tokio::test]
async fn m5_a_stub_stays_a_stub_when_a_version_that_fits_arrives() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut big = raw("uid-1", "t", "x");
    big.to = vec!["t".repeat(65 * 1024)];
    let Ingested::TooLarge { id, .. } = repo.ingest(me.id, big).await.unwrap() else {
        panic!();
    };
    let again = repo
        .ingest(me.id, raw("uid-1", "t", "now it fits"))
        .await
        .unwrap();
    assert_eq!(again, Ingested::Duplicate(id));
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.body_text, "", "the stated limit in DESIGN.md");
}

#[tokio::test]
async fn m6_a_reply_to_a_stub_finds_it() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut big = raw("uid-1", "t", "x");
    big.to = vec!["t".repeat(65 * 1024)];
    big.links.push(message_id("big@x"));
    let Ingested::TooLarge { id, .. } = repo.ingest(me.id, big).await.unwrap() else {
        panic!();
    };
    let mut reply = raw("uid-2", "t", "about that");
    reply.in_reply_to = Some(message_id("big@x"));
    let reply = stored(&repo, me.id, reply).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, reply).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, Some(id.to_string()));
    assert_eq!(
        repo.message(conv, id).await.unwrap().aliases,
        [alias("uid-1")]
    );
}

#[tokio::test]
async fn k5_a_summary_says_how_large_and_0_means_it_is_not() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    stored(&repo, me.id, raw("uid-1", "t", "small")).await;
    let mut big = raw("uid-2", "t", "x");
    big.to = vec!["t".repeat(65 * 1024)];
    let Ingested::TooLarge { size, .. } = repo.ingest(me.id, big).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let page = repo.messages(conv, None).await.unwrap().items;
    let mut sizes: Vec<u64> = page.iter().map(|m| m.too_large).collect();
    sizes.sort_unstable();
    assert_eq!(sizes, [0, size as u64]);
}

#[test]
fn k4_the_contract_version_is_0_3() {
    assert_eq!(CONTRACT_VERSION, "0.3");
}
