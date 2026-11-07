//! With a guard that says "not open", a report of a tool call is kept as `cancelled` unless its sender
//! is a person or plain software. Without a guard nothing is stopped, so a report stays as it is.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{account, raw, setup};
use fs_messenger_core::{
    EngineGuard, Ingest, Ingested, Result, SenderKind, SenderLookup, ToolCall, ToolCallDecision,
    ToolCallKind, ToolCallOutcome, ToolCallState,
};
use fs_store::ObjectId;

/// Shut, and sure of whom it knows by the first letters of the address.
struct Shut;

#[async_trait]
impl EngineGuard for Shut {
    async fn decide(&self, _: ObjectId, _: ObjectId, _: ToolCallDecision) -> Result<()> {
        Ok(())
    }
    async fn stop_conversation(&self, _: ObjectId) -> Result<()> {
        Ok(())
    }
    async fn open_for_reports(&self, _: &str) -> bool {
        false
    }
}

#[async_trait]
impl SenderLookup for Shut {
    async fn sender_kind(&self, sender: &str) -> SenderKind {
        match sender.chars().next() {
            Some('p') => SenderKind::Person,
            Some('s') => SenderKind::Software,
            Some('a') => SenderKind::Ai,
            _ => SenderKind::Unknown,
        }
    }
}

fn working() -> ToolCall {
    ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state: ToolCallState::Working,
        ..ToolCall::default()
    }
}

async fn report_from(sender: &str, guarded: bool) -> ToolCall {
    let (repo, _) = setup().await;
    if guarded {
        let shut = Arc::new(Shut);
        repo.install_guard(shut.clone(), shut).unwrap();
    }
    let me = account(&repo, "me@example.org").await;
    let mut message = raw("m1", "t", "hi");
    message.sender = sender.into();
    let Ingested::Stored(id) = repo.ingest(me.id, message).await.unwrap() else {
        panic!()
    };
    repo.record_tool_call(id, "r", working()).await.unwrap()
}

#[tokio::test]
async fn a_shut_guard_cancels_what_ai_and_unknown_report() {
    for sender in ["ai@x", "unknown@x"] {
        let call = report_from(sender, true).await;
        assert_eq!(call.state, ToolCallState::Settled, "{sender}");
        assert_eq!(call.outcome, ToolCallOutcome::Cancelled, "{sender}");
    }
}

#[tokio::test]
async fn a_person_or_plain_software_is_not_cancelled() {
    for sender in ["person@x", "software@x"] {
        let call = report_from(sender, true).await;
        assert_eq!(call.state, ToolCallState::Working, "{sender}");
    }
}

#[tokio::test]
async fn without_a_guard_nothing_is_stopped() {
    let call = report_from("ai@x", false).await;
    assert_eq!(call.state, ToolCallState::Working);
}
