//! A message that arrives with a tool call in it goes through the same question as a later report
//! (`record_tool_call`): after a stop, from an AI or a stranger, the call is kept `cancelled`; before
//! the stop and after a `reopen`, it is `working`. A stop that comes in while the message is being
//! written still wins, because the core asks the guard again after the write.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::rigs::Rig;
use common::{AI, OTHER_AI, World};
use fs_messenger_core::{
    Encryption, ExternalId, Ingest, Ingested, Kind, Messenger, MessengerRepo, RawMessage,
    RawToolCall, ToolCall, ToolCallKind, ToolCallOutcome, ToolCallState,
};
use fs_messenger_guard::{Guard, Method, Verdict};
use fs_objects::ObjectId;

fn working() -> ToolCall {
    ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state: ToolCallState::Working,
        ..ToolCall::default()
    }
}

/// A message from `sender` that already carries one call in the state `working`.
async fn arrive(repo: &MessengerRepo, account: ObjectId, id: &str, sender: &str) -> ObjectId {
    let mut raw = RawMessage::new(
        ExternalId::new("uid", id),
        ExternalId::new("thread", "t"),
        Kind::Chat,
        sender,
        "hello",
    );
    raw.encryption = Encryption::Mls;
    raw.tool_calls = vec![RawToolCall {
        agent_ref: "r".into(),
        call: working(),
    }];
    let Ingested::Stored(note) = repo.ingest(account, raw).await.unwrap() else {
        panic!("not stored")
    };
    note
}

async fn calls(repo: &MessengerRepo, note: ObjectId) -> Vec<ToolCall> {
    for c in repo.conversations(None, false, None).await.unwrap().items {
        if let Ok(m) = repo.message(c.id, note).await {
            return m.tool_calls;
        }
    }
    panic!("message not found");
}

fn is_cancelled(call: &ToolCall) -> bool {
    call.state == ToolCallState::Settled && call.outcome == ToolCallOutcome::Cancelled
}

#[tokio::test]
async fn ingest_before_a_stop_keeps_the_call_working() {
    let w = World::working().await;
    let account = w.account("a@local", "matrix").await;
    let note = arrive(&w.repo, account.id, "m1", AI).await;
    let held = calls(&w.repo, note).await;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].state, ToolCallState::Working, "{held:?}");
}

#[tokio::test]
async fn ingest_after_a_stop_is_cancelled_for_an_ai_and_for_strangers() {
    let w = World::working().await;
    let account = w.account("a@local", "matrix").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    for (n, sender) in [AI, OTHER_AI, "x@y"].into_iter().enumerate() {
        let note = arrive(&w.repo, account.id, &format!("m{n}"), sender).await;
        let held = calls(&w.repo, note).await;
        assert!(held.iter().all(is_cancelled), "{sender}: {held:?}");
        assert!(held[0].options.is_empty() && held[0].asked_of.is_empty());
    }
}

#[tokio::test]
async fn ingest_after_a_reopen_is_working_again() {
    let w = World::working().await;
    let account = w.account("a@local", "matrix").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    let note = arrive(&w.repo, account.id, "m1", AI).await;
    assert_eq!(calls(&w.repo, note).await[0].state, ToolCallState::Working);
}

/// The message asks the guard under the core's write lock and stands still there. The stop comes in
/// meanwhile: its settling has to wait for that lock, and reads nothing of this message. The answer
/// the message holds is the old one ("open"), so only the second question after the write can cancel.
#[tokio::test]
async fn a_message_met_by_a_stop_between_question_and_write_is_cancelled() {
    let r = Rig::open().await;
    let account = r
        .repo
        .add_account(fs_messenger_core::AccountSetup {
            address: "me@x".into(),
            kind: "matrix".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    let before = arrive(&r.repo, account.id, "m0", AI).await;
    assert_eq!(
        calls(&r.repo, before).await[0].state,
        ToolCallState::Working
    );

    // The first load is the lookup of who sends; let it pass, and hold the one after it.
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    let repo = r.repo.clone();
    let id = account.id;
    let ingest = tokio::spawn(async move { arrive(&repo, id, "m1", AI).await });
    r.faulty.entered.notified().await;
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    r.faulty.go.notify_one();
    // Now the core is under its write lock, asking whether the program is open.
    tokio::time::timeout(Duration::from_secs(5), r.faulty.entered.notified())
        .await
        .expect("the message never asked the guard whether the program is open");

    let engine = r.engine.clone();
    let proof = r.proof(Method::Stop);
    let stop = tokio::spawn(async move { engine.stop(&proof).await });
    // Within the guard's `sender_wait` (100 ms in the rig): the held answer must still read "open".
    common::rigs::eventually("the stop is in force", async || {
        !matches!(r.read_check().await, Verdict::Allow(_))
    })
    .await;
    r.faulty.go.notify_one();

    let note = ingest.await.unwrap();
    stop.await.unwrap().unwrap();
    let held = calls(&r.repo, note).await;
    assert!(held.iter().all(is_cancelled), "{held:?}");
    assert!(calls(&r.repo, before).await.iter().all(is_cancelled));
}
