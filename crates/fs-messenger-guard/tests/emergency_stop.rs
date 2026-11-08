//! The emergency stop: at once, persisted, and only a person opens it again.

mod common;

use common::{AI, OWNER, World, args};
use fs_messenger_core::{
    Error, MessageAction, Messenger, ToolCallOutcome, ToolCallRisk, ToolCallState,
};
use fs_messenger_guard::{AiGrant, Gate, Guard, GuardError, GuardMode, Refusal, Verdict};
use serde_json::json;
#[tokio::test]
async fn stop_refuses_every_ai_takes_all_releases_and_settles_all_calls() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let open = w.ask("message.delete", args("/q")).await;
    let approved = w.ask("message.delete", args("/a")).await;
    let d = w
        .owner
        .decide(&w.identity, approved.message, &approved.call, "allow-once");
    w.repo
        .act(
            approved.conversation,
            approved.message,
            MessageAction::Decide(d),
        )
        .await
        .unwrap();

    let report = w.engine.stop(&w.stop_proof()).await.unwrap();
    assert_eq!(report.not_persisted, None);
    assert_eq!(report.calls_cancelled, 2);
    assert!(report.messenger_attached);

    let state = w.engine.state().await;
    assert_eq!(state.mode, GuardMode::Stopped);
    assert!(state.grants.is_empty(), "expected empty");
    assert_eq!(state.stopped_by, OWNER);
    assert!(state.stopped_at.is_some());
    assert_eq!(
        state.invited,
        [AI],
        "they stay invited; they are only shut out"
    );

    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert_eq!(
        w.check("message.delete", args("/a")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    for q in [&open, &approved] {
        let shown = w.shown(q).await;
        assert_eq!(shown.state, ToolCallState::Settled);
        assert_eq!(shown.outcome, ToolCallOutcome::Cancelled);
        assert!(shown.options.is_empty(), "expected empty");
    }
}

#[tokio::test]
async fn a_late_decision_after_the_stop_gets_a_problem() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/q")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let before = w.record().await;
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(matches!(
        result,
        Err(Error::DecisionRefused {
            problem: fs_messenger_core::DecisionProblem::Settled,
            ..
        })
    ));
    assert_eq!(w.record().await, before);
}

#[tokio::test]
async fn a_restart_stays_stopped_with_the_releases_gone() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let w = w.restart().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert!(w.engine.state().await.grants.is_empty(), "expected empty");
    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
}

#[tokio::test]
async fn reopen_needs_a_proof_and_the_releases_stay_taken_also_a_tampered_one() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let mut bad = w.reopen_proof();
    bad.proof[0] ^= 1;
    assert!(w.engine.reopen(&bad).await.is_err());
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);

    w.tamper(|doc| {
        doc["grants"] = json!([{ "profile": AI, "risk": "readsOnly", "program": "message" }]);
    })
    .await;
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    let state = w.engine.state().await;
    assert_eq!(state.mode, GuardMode::Open);
    assert!(
        state.grants.is_empty(),
        "a release does not come back through a reopen"
    );
    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
    assert!(matches!(
        w.engine.reopen(&w.reopen_proof()).await,
        Err(GuardError::Refused(Refusal::AlreadyOpen))
    ));
}

#[tokio::test]
async fn a_stop_cannot_lose_a_race_with_decisions_or_a_check() {
    let w = World::working().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let engine = w.engine.clone();
    let input = fs_messenger_guard::Arguments::from_value(&args("/")).unwrap();
    let runner = tokio::spawn(async move {
        let caller = fs_messenger_guard::Caller::new(AI);
        for _ in 0..200 {
            let _ = engine.check(&caller, "message.open", &input).await;
            tokio::task::yield_now().await;
        }
    });
    tokio::task::yield_now().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    runner.await.unwrap();
    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    let grant = AiGrant {
        profile: AI.into(),
        risk: ToolCallRisk::ReadsOnly,
        program: "message".into(),
    };
    assert!(
        w.engine
            .release(&grant, &w.release_proof(AI, "readsOnly"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn after_a_stop_this_process_stays_stopped_even_if_a_record_says_open() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.tamper(|doc| {
        doc["mode"] = json!("open");
        doc["calls"][0]["stage"] = json!("question");
        doc["calls"][0]["revision"] = json!(1);
        doc["calls"][0]["expiresAt"] = json!("2999-01-01T00:00:00Z");
    })
    .await;
    assert_eq!(
        w.read("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(click))
        .await;
    assert!(
        matches!(result, Err(Error::DecisionRefused { .. })),
        "{result:?}"
    );
    assert_eq!(w.engine.admit_message(AI).await, Err(Refusal::Stopped));
}
