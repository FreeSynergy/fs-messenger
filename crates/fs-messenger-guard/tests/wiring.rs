//! How a guard gets in: once, routed, and without one the safe default.

mod common;

use std::sync::Arc;

use common::{AI, OWNER, World, args, encrypted, engine_on};
use fs_messenger_core::{
    AccountSetup, Ingest, Kind, MessageAction, Messenger, MessengerConfig, MessengerRepo,
    RawMessage, ToolCallDecision,
};
use fs_store::{ObjectId, TursoStore};
use serde_json::json;
#[tokio::test]
async fn a_second_guard_cannot_be_installed() {
    let w = World::open().await;
    let store = Arc::new(
        TursoStore::open_with(w.dir.path().join("other.db"), encrypted())
            .await
            .unwrap(),
    );
    let (intruder, _) = engine_on(&store, &w.clock, &w.dir.path().join("other.stopped"));
    assert!(intruder.attach(&w.repo).is_err());
    let again = w.engine.attach(&w.repo);
    assert!(again.is_err(), "and the same one not twice");
}

#[tokio::test]
async fn the_decide_route_goes_to_the_guard_and_needs_a_proof() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let mut d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    d.proof = vec![0; 64];
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(matches!(
        result,
        Err(fs_messenger_core::Error::DecisionRefused { .. })
    ));
    let caps = w.repo.capabilities();
    assert!(caps.is_supported("toolcall.decide"));
    assert!(caps.is_supported("turn.stop"));
}

#[tokio::test]
async fn without_a_guard_decide_and_stop_answer_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("m.db"), encrypted())
            .await
            .unwrap(),
    );
    let repo = MessengerRepo::open_turso(store, MessengerConfig::new(OWNER))
        .await
        .unwrap();
    let id = ObjectId::new();
    let decide = MessageAction::Decide(ToolCallDecision {
        tool_call: "x".into(),
        ..ToolCallDecision::default()
    });
    assert!(matches!(
        repo.act(id, id, decide).await,
        Err(fs_messenger_core::Error::Unsupported(u)) if u.capability() == "toolcall.decide"
    ));
    assert!(matches!(
        repo.stop(id).await,
        Err(fs_messenger_core::Error::Unsupported(u)) if u.capability() == "turn.stop"
    ));
    let caps = repo.capabilities();
    assert!(!caps.is_supported("toolcall.decide"));
    assert!(!caps.is_supported("turn.stop"));
}

#[tokio::test]
async fn without_a_guard_an_agent_account_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("m.db"), encrypted())
            .await
            .unwrap(),
    );
    let repo = MessengerRepo::open_turso(store, MessengerConfig::new(OWNER))
        .await
        .unwrap();
    let account = repo
        .add_account(AccountSetup {
            address: "agents@local".into(),
            kind: "agent".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    let raw = RawMessage::new(
        fs_messenger_core::ExternalId::new("uid", "1"),
        fs_messenger_core::ExternalId::new("thread", "1"),
        Kind::Chat,
        AI,
        "hello",
    );
    assert!(matches!(
        repo.ingest(account.id, raw).await,
        Err(fs_messenger_core::Error::GuardRefused(_))
    ));
    let _ = json!({});
}
