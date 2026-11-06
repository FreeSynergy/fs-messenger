//! The stop and the providers: each answer reported, none can hold it up; a stop per conversation.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::{AI, World, args};
use fs_messenger_core::{
    Error, Ingest, MessageAction, Messenger, Provider, ToolCall, ToolCallKind, ToolCallOutcome,
    ToolCallState, TurnScope, TurnStopOutcome,
};
use fs_messenger_guard::{Gate, Guard, GuardMode, Refusal, Verdict};
use tokio::sync::Notify;
struct CanStop(Arc<Notify>);

#[async_trait]
impl Provider for CanStop {
    async fn stop_turns(&self, _scope: &TurnScope) -> fs_messenger_core::Result<()> {
        self.0.notify_one();
        Ok(())
    }
}

struct Hangs;

#[async_trait]
impl Provider for Hangs {
    async fn stop_turns(&self, _scope: &TurnScope) -> fs_messenger_core::Result<()> {
        std::future::pending().await
    }
}

struct Panics;

#[async_trait]
impl Provider for Panics {
    async fn stop_turns(&self, _scope: &TurnScope) -> fs_messenger_core::Result<()> {
        panic!("a provider falls over");
    }
}

struct CannotStop;

#[async_trait]
impl Provider for CannotStop {}

#[tokio::test]
async fn providers_are_asked_and_each_answer_is_in_the_report() {
    let w = World::open().await;
    let can = w.account("a@x", "mail").await;
    let cannot = w.account("b@x", "mail").await;
    let notified = Arc::new(Notify::new());
    w.repo
        .register_provider(can.id, Arc::new(CanStop(notified.clone())));
    w.repo.register_provider(cannot.id, Arc::new(CannotStop));
    let report = w.engine.stop(&w.stop_proof()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), notified.notified())
        .await
        .unwrap();
    let by = |id| {
        report
            .turns
            .iter()
            .find(|t| t.account == id)
            .unwrap()
            .outcome
            .clone()
    };
    assert_eq!(by(can.id), TurnStopOutcome::Ended);
    assert!(
        matches!(by(cannot.id), TurnStopOutcome::Unsupported(_)),
        "a provider that cannot is reported, not hidden"
    );
}

#[tokio::test]
async fn a_provider_that_hangs_or_panics_cannot_hold_up_the_stop() {
    let dir_w = World::open().await;
    let w = dir_w;
    let hang = w.account("h@x", "mail").await;
    let panic = w.account("p@x", "mail").await;
    w.repo.register_provider(hang.id, Arc::new(Hangs));
    w.repo.register_provider(panic.id, Arc::new(Panics));
    let engine = w.engine.clone();
    let proof = w.stop_proof();
    let stop = tokio::spawn(async move { engine.stop(&proof).await });
    // Well before the provider's time is up, `check` already refuses.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert_eq!(
        World::doc_mode(&w.record().await.unwrap()),
        "stopped",
        "written already"
    );
    let report = tokio::time::timeout(Duration::from_secs(10), stop)
        .await
        .expect("the stop ends")
        .unwrap()
        .unwrap();
    assert!(
        report
            .turns
            .iter()
            .all(|t| t.outcome == TurnStopOutcome::NoAnswer)
    );
    assert_eq!(report.turns.len(), 2);
}

#[tokio::test]
async fn conversation_stop_settles_the_calls_there_and_only_there() {
    let w = World::with_ai().await;
    let mail = w.account("me@example.org", "mail").await;
    let notified = Arc::new(Notify::new());
    w.repo
        .register_provider(mail.id, Arc::new(CanStop(notified.clone())));
    let fs_messenger_core::Ingested::Stored(note) = w.deliver(mail.id, "m1", "x@y").await.unwrap()
    else {
        panic!()
    };
    let here = w.repo.conversations(None, false, None).await.unwrap().items[0].id;
    let report = ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state: ToolCallState::Working,
        ..ToolCall::default()
    };
    w.repo.record_tool_call(note, "t1", report).await.unwrap();
    let caller = fs_messenger_guard::Caller::in_conversation(AI, here);
    let input = fs_messenger_guard::Arguments::from_value(&args("/q")).unwrap();
    let Verdict::Ask(q) = w.engine.check(&caller, "message.delete", &input).await else {
        panic!()
    };
    assert_eq!(q.conversation, here);
    let elsewhere = w.ask("message.delete", args("/elsewhere")).await;
    assert_ne!(elsewhere.conversation, here);

    w.repo.stop(here).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), notified.notified())
        .await
        .unwrap();

    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Cancelled);
    let agents = w.repo.message(here, note).await.unwrap().tool_calls;
    assert_eq!(
        agents[0].outcome,
        ToolCallOutcome::Cancelled,
        "the agent's own report too"
    );
    assert_eq!(w.shown(&elsewhere).await.state, ToolCallState::Question);
    assert_eq!(
        w.engine.state().await.mode,
        GuardMode::Open,
        "only this conversation"
    );
    // A click on what was settled gets a problem.
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    assert!(
        w.repo
            .act(q.conversation, q.message, MessageAction::Decide(d))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn conversation_stop_reports_a_provider_that_cannot_end_a_turn() {
    let w = World::with_ai().await;
    let mail = w.account("me@example.org", "mail").await;
    w.repo.register_provider(mail.id, Arc::new(CannotStop));
    w.deliver(mail.id, "m1", "x@y").await.unwrap();
    let here = w.repo.conversations(None, false, None).await.unwrap().items[0].id;
    let result = w.repo.stop(here).await;
    assert!(
        matches!(&result, Err(Error::Unsupported(u)) if u.capability() == "turn.stop"),
        "{result:?}"
    );
    let unknown = fs_store::ObjectId::new();
    assert!(w.repo.stop(unknown).await.is_err());
}
