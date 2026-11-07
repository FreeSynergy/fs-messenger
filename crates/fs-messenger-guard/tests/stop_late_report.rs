//! A report of a tool call that arrives after a stop is kept as `cancelled` when its sender is not
//! known to be a person: an invited AI, an uninvited one, and one nobody knows. Before the stop and
//! after a `reopen` a report is shown as `working`, as always. A report never carries a right.

mod common;

use common::{AI, OTHER_AI, World};
use fs_messenger_core::{
    Ingest, Ingested, Messenger, ToolCall, ToolCallKind, ToolCallOutcome, ToolCallState,
};
use fs_messenger_guard::Guard;
use fs_objects::ObjectId;

fn report(state: ToolCallState) -> ToolCall {
    ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state,
        ..ToolCall::default()
    }
}

async fn message_from(w: &World, id: &str, sender: &str) -> ObjectId {
    let account = w.account(&format!("{id}@local"), "matrix").await;
    let Ingested::Stored(note) = w.deliver(account.id, id, sender).await.unwrap() else {
        panic!()
    };
    note
}

async fn shown(w: &World, note: ObjectId) -> Vec<ToolCall> {
    let conversation = w.repo.conversations(None, false, None).await.unwrap().items;
    for c in conversation {
        if let Ok(m) = w.repo.message(c.id, note).await {
            return m.tool_calls;
        }
    }
    panic!("message not found");
}

fn is_cancelled(call: &ToolCall) -> bool {
    call.state == ToolCallState::Settled && call.outcome == ToolCallOutcome::Cancelled
}

#[tokio::test]
async fn before_a_stop_a_report_from_an_invited_ai_stays_working() {
    let w = World::with_ai().await;
    let note = message_from(&w, "m1", AI).await;
    let stored = w
        .repo
        .record_tool_call(note, "r1", report(ToolCallState::Working))
        .await
        .unwrap();
    assert_eq!(stored.state, ToolCallState::Working);
}

#[tokio::test]
async fn after_a_stop_a_report_from_an_invited_ai_is_cancelled_new_or_not() {
    let w = World::with_ai().await;
    let note = message_from(&w, "m1", AI).await;
    w.repo
        .record_tool_call(note, "old", report(ToolCallState::Working))
        .await
        .unwrap();
    w.engine.stop(&w.stop_proof()).await.unwrap();

    let new = w
        .repo
        .record_tool_call(note, "new", report(ToolCallState::Working))
        .await
        .unwrap();
    assert!(is_cancelled(&new), "{new:?}");
    let again = w
        .repo
        .record_tool_call(note, "old", report(ToolCallState::Forming))
        .await
        .unwrap();
    assert!(is_cancelled(&again), "{again:?}");
    assert!(shown(&w, note).await.iter().all(is_cancelled));
}

#[tokio::test]
async fn after_a_stop_nobody_unknown_or_uninvited_gets_a_working_report_either() {
    let w = World::with_ai().await;
    let uninvited = message_from(&w, "m1", OTHER_AI).await;
    let unknown = message_from(&w, "m2", "x@y").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    for note in [uninvited, unknown] {
        let stored = w
            .repo
            .record_tool_call(note, "r", report(ToolCallState::Working))
            .await
            .unwrap();
        assert!(is_cancelled(&stored), "{stored:?}");
        assert!(stored.options.is_empty() && stored.asked_of.is_empty());
    }
}

#[tokio::test]
async fn once_the_program_is_open_again_reports_are_working_again() {
    let w = World::with_ai().await;
    let note = message_from(&w, "m1", AI).await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    let stored = w
        .repo
        .record_tool_call(note, "r", report(ToolCallState::Working))
        .await
        .unwrap();
    assert_eq!(stored.state, ToolCallState::Working);
}
