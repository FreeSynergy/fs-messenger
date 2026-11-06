//! The emergency stop: at once, persisted, and only a person opens it again.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::{AI, Device, FRIEND, OWNER, World, args};
use fs_messenger_core::{
    Error, Ingest, MessageAction, Messenger, Provider, ToolCall, ToolCallKind, ToolCallOutcome,
    ToolCallRisk, ToolCallState, TurnScope, TurnStopOutcome,
};
use fs_messenger_guard::{AiGrant, Gate, Guard, GuardError, GuardMode, Method, Refusal, Verdict};
use serde_json::json;
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
async fn stop_refuses_every_ai_takes_all_releases_and_settles_all_calls() {
    let w = World::with_ai().await;
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
    assert!(state.grants.is_empty());
    assert_eq!(state.stopped_by, OWNER);
    assert!(state.stopped_at.is_some());
    assert_eq!(
        state.invited,
        [AI],
        "they stay invited; they are only shut out"
    );

    assert_eq!(
        w.check("message.open", args("/")).await,
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
        assert!(shown.options.is_empty());
    }
}

#[tokio::test]
async fn a_late_decision_after_the_stop_gets_a_problem() {
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let w = w.restart().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert!(w.engine.state().await.grants.is_empty());
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
}

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
async fn only_the_owner_may_pull_it_or_open_it_and_not_a_person_with_a_key_of_their_own() {
    let w = World::open().await;
    let friend = Device::new(FRIEND, 2);
    let key: String = friend.public().iter().map(|b| format!("{b:02x}")).collect();
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();

    let refused = w
        .engine
        .stop(&friend.proof(&w.identity, Method::Stop, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    assert_eq!(
        w.engine.state().await.mode,
        GuardMode::Open,
        "no effect at all"
    );

    w.engine.stop(&w.stop_proof()).await.unwrap();
    let refused = w
        .engine
        .reopen(&friend.proof(&w.identity, Method::Reopen, &[]))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
}

#[tokio::test]
async fn an_ai_can_neither_pull_it_nor_open_it_even_with_a_key_and_as_owner_by_a_tampered_record() {
    let w = World::open().await;
    let ai = Device::new(AI, 9);
    let key: String = ai.public().iter().map(|b| format!("{b:02x}")).collect();
    w.invite(AI).await;
    // Someone with the store in hand gives the AI a key.
    w.tamper(|doc| {
        doc["devices"][AI] = json!([{ "keyType": "ed25519", "publicKey": key }]);
    })
    .await;
    let refused = w
        .engine
        .stop(&ai.proof(&w.identity, Method::Stop, &[]))
        .await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let refused = w
        .engine
        .reopen(&ai.proof(&w.identity, Method::Reopen, &[]))
        .await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);

    // Also when it is the owner on paper: being marked AI comes first.
    w.tamper(|doc| {
        doc["invited"][OWNER] = json!({ "by": OWNER, "at": "2026-01-01T00:00:00Z" });
    })
    .await;
    let refused = w.engine.reopen(&w.reopen_proof()).await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::SignerIsAi))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn reopen_needs_a_proof_and_the_releases_stay_taken_also_a_tampered_one() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let mut bad = w.reopen_proof();
    bad.proof[0] ^= 1;
    assert!(w.engine.reopen(&bad).await.is_err());
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);

    w.tamper(|doc| doc["grants"] = json!([{ "profile": AI, "risk": "readsOnly" }]))
        .await;
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    let state = w.engine.state().await;
    assert_eq!(state.mode, GuardMode::Open);
    assert!(
        state.grants.is_empty(),
        "a release does not come back through a reopen"
    );
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Ask(_)
    ));
    assert!(matches!(
        w.engine.reopen(&w.reopen_proof()).await,
        Err(GuardError::Refused(Refusal::AlreadyOpen))
    ));
}

#[tokio::test]
async fn it_works_with_no_messenger_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        fs_store::TursoStore::open_with(dir.path().join("g.db"), common::encrypted())
            .await
            .unwrap(),
    );
    let clock = Arc::new(common::TestClock::default());
    let (engine, enrolment) = common::engine_on(&store, &clock);
    let owner = Device::new(OWNER, 1);
    enrolment.enrol_owner(&owner.public()).await.unwrap();
    let identity = engine.engine_identity().await.unwrap();
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    assert_eq!(engine.state().await.mode, GuardMode::Open);

    let report = engine
        .stop(&owner.proof(&identity, Method::Stop, &[]))
        .await
        .unwrap();
    assert!(!report.messenger_attached, "said, not hidden");
    assert_eq!(report.not_persisted, None);
    assert_eq!(engine.state().await.mode, GuardMode::Stopped);
    // And nobody can be asked without a messenger, so nothing gets through that way either.
    engine
        .reopen(&owner.proof(&identity, Method::Reopen, &[]))
        .await
        .unwrap();
    let proof = owner.proof(&identity, Method::Invite, &[AI.as_bytes()]);
    engine.invite(AI, &proof).await.unwrap();
    let verdict = engine
        .check(
            &fs_messenger_guard::Caller::new(AI),
            "message.delete",
            &fs_messenger_guard::Arguments::none(),
        )
        .await;
    assert_eq!(verdict, Verdict::Refuse(Refusal::NoWayToAsk));
}

#[tokio::test]
async fn a_group_stop_answers_unsupported() {
    let w = World::open().await;
    let result = w.engine.stop_group("g", &w.stop_proof()).await;
    assert!(
        matches!(result, Err(GuardError::Unsupported(u)) if u.capability() == "guard.stop.group")
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    use fs_messenger_guard::{OwnerOnly, StopPermission};
    assert!(OwnerOnly.may_stop_group("g", OWNER).is_err());
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

#[tokio::test]
async fn a_stop_cannot_lose_a_race_with_decisions_or_a_check() {
    let w = World::with_ai().await;
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
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    runner.await.unwrap();
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    let grant = AiGrant {
        profile: AI.into(),
        risk: ToolCallRisk::ReadsOnly,
    };
    assert!(
        w.engine
            .release(&grant, &w.release_proof(AI, "readsOnly"))
            .await
            .is_err()
    );
}
