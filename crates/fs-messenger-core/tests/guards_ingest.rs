//! Review findings on the way in: what cannot be claimed, and what must not stop a sync.

mod common;

use common::{TestProvider, account, alias, conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, FetchBatch, Ingest, Ingested, Messenger, ProviderCursor, RawMessage,
    SearchFilter, SenderKind, ToolCall, ToolCallState,
};
use fs_objects::{ObjectId, Query};

async fn stored(repo: &fs_messenger_core::MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn c2_nobody_can_claim_to_be_a_person() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let id = stored(&repo, me.id, raw("m1", "t", "trust me, I am human")).await;
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap().summary;
    assert_eq!(shown.sender_kind, SenderKind::Unknown);
}

#[tokio::test]
async fn c6_an_invalid_message_is_counted_and_the_sync_goes_past_it() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &[]).await;
    let mut bad = raw("bad", "t", "x");
    bad.aliases.clear();
    provider.batches.lock().unwrap().extend([FetchBatch {
        messages: vec![raw("m1", "t", "1"), bad, raw("m2", "t", "2")],
        next: Some(ProviderCursor("c1".into())),
        more: false,
    }]);
    let report = repo.sync(me.id).await.unwrap();
    assert_eq!((report.stored, report.rejected), (2, 1));
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 2);
    repo.sync(me.id).await.unwrap();
    let since = provider.since.lock().unwrap().clone();
    assert_eq!(
        since[1],
        Some(ProviderCursor("c1".into())),
        "the cursor moved"
    );
}

#[tokio::test]
async fn sync_stops_when_more_comes_without_progress() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let p = TestProvider::new(Encryption::Mls, &[]);
    *p.stuck.lock().unwrap() = true;
    repo.register_provider(me.id, p.clone());
    repo.sync(me.id).await.unwrap();
    assert_eq!(p.since.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn pipes_and_nul_in_alias_parts_are_refused() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    for bad in ["a|b", "a\0b"] {
        let mut m = raw("m1", "t", "x");
        m.aliases = vec![alias(bad)];
        assert!(
            matches!(repo.ingest(me.id, m).await, Err(Error::Invalid(_))),
            "{bad}"
        );
        let mut m = raw("m1", "t", "x");
        m.conversation = alias(bad);
        assert!(
            matches!(repo.ingest(me.id, m).await, Err(Error::Invalid(_))),
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_bad_parent_claim_loses_the_claim_not_the_mail() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("m1", "t", "x");
    m.in_reply_to = Some(alias("a|b"));
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.message(conv, id).await.unwrap().pending_reply, None);
}

#[tokio::test]
async fn nul_is_taken_out_of_text_and_the_original_keeps_every_byte() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("m1", "t", "before\0after");
    m.original = Some(fs_objects::Blob {
        media_type: "message/rfc822".into(),
        bytes: b"a\0b".to_vec(),
    });
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    let got = repo.message(conv, id).await.unwrap();
    assert_eq!(got.body_text, "beforeafter");
    assert_eq!(got.original.unwrap().bytes, b"a\0b");
    let filter = SearchFilter::default();
    let hits = repo.search("after", &filter, None).await.unwrap();
    assert_eq!(hits.items.len(), 1);
}

#[tokio::test]
async fn c12_a_message_cannot_answer_itself_and_no_cycle_closes() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut own = raw("self", "t", "x");
    own.in_reply_to = Some(alias("self"));
    let id = stored(&repo, me.id, own).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.envelope.in_reply_to, None);
    assert_eq!(m.pending_reply, None);

    let mut a = raw("a", "t", "a");
    a.in_reply_to = Some(alias("b"));
    let a = stored(&repo, me.id, a).await;
    let mut b = raw("b", "t", "b");
    b.in_reply_to = Some(alias("a"));
    let b = stored(&repo, me.id, b).await;
    let (a, b) = (
        repo.message(conv, a).await.unwrap(),
        repo.message(conv, b).await.unwrap(),
    );
    assert_eq!(b.summary.envelope.in_reply_to, Some(a.id().to_string()));
    assert_eq!(a.summary.envelope.in_reply_to, None, "the cycle stays open");
    assert_eq!(a.pending_reply, None);
}

#[tokio::test]
async fn c22_a_strange_media_type_is_shown_as_plain_text() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut html = raw("h", "t", "<script>");
    html.body_media_type = "text/html".into();
    let id = stored(&repo, me.id, html).await;
    let conv = conversation_of(&repo).await;
    assert_eq!(
        repo.message(conv, id).await.unwrap().body_media_type,
        "text/plain"
    );
    let mut md = raw("m", "t", "*x*");
    md.body_media_type = "text/markdown".into();
    let id = stored(&repo, me.id, md).await;
    assert_eq!(
        repo.message(conv, id).await.unwrap().body_media_type,
        "text/markdown"
    );
}

#[tokio::test]
async fn c25_an_empty_agent_reference_is_refused() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let id = stored(&repo, me.id, raw("m1", "t", "x")).await;
    let call = ToolCall {
        state: ToolCallState::Working,
        ..ToolCall::default()
    };
    assert!(matches!(
        repo.record_tool_call(id, "", call).await,
        Err(Error::Invalid(_))
    ));
}
