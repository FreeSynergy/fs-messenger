//! What the review named about the ways in: reports, account kinds, who gets to install, one engine
//! per store, what the log says, and what must not be lost.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{AI, OWNER, World, args, encrypted, engine_on};
use fs_messenger_core::{
    AccountSetup, Error, Ingest, Ingested, Messenger, MessengerConfig, MessengerRepo, Provider,
    ToolCall, ToolCallKind, ToolCallState,
};
use fs_messenger_guard::{GuardError, TursoGuardRepository};
use fs_objects::TursoStore;
fn report(state: ToolCallState) -> ToolCall {
    ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state,
        ..ToolCall::default()
    }
}

#[tokio::test]
async fn a_report_cannot_land_beside_a_persons_question() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let refused = w
        .repo
        .record_tool_call(q.message, "r1", report(ToolCallState::Working))
        .await;
    assert!(
        matches!(refused, Err(Error::GuardRefused(_))),
        "{refused:?}"
    );
    assert_eq!(
        w.repo
            .message(q.conversation, q.message)
            .await
            .unwrap()
            .tool_calls
            .len(),
        1
    );

    let account = w.account("ai@local", "matrix").await;
    let Ingested::Stored(note) = w.deliver(account.id, "m1", AI).await.unwrap() else {
        panic!()
    };
    w.repo
        .record_tool_call(note, "r2", report(ToolCallState::Working))
        .await
        .unwrap();
}

#[tokio::test]
async fn only_transports_are_kinds_of_account_and_unknown_kinds_are_invalid() {
    let w = World::open().await;
    for unknown in ["ai", "llm", "ag ent", "agents", "agent", ""] {
        let result = w
            .repo
            .add_account(AccountSetup {
                address: "b@x".into(),
                kind: unknown.into(),
                secret: String::new(),
            })
            .await;
        assert!(matches!(result, Err(Error::Invalid(_))), "{unknown:?}");
    }
    for known in ["mail", "matrix", "mimi", "MAIL"] {
        let address = format!("{known}-{}@x", known.len() + usize::from(known == "MAIL"));
        let setup = AccountSetup {
            address,
            kind: known.into(),
            secret: String::new(),
        };
        if known != "MAIL" {
            w.repo.add_account(setup).await.unwrap();
        } else {
            assert!(matches!(w.repo.add_account(setup).await, Ok(a) if a.kind == "mail"));
        }
    }
}

#[tokio::test]
async fn an_account_of_type_agent_is_refused_in_every_spelling() {
    let w = World::open().await;
    for spelt in ["agent", "Agent", " agent", "AGENT "] {
        let result = w
            .repo
            .add_account(AccountSetup {
                address: "agents@local".into(),
                kind: spelt.into(),
                secret: String::new(),
            })
            .await;
        assert!(matches!(result, Err(Error::Invalid(_))), "{spelt:?}");
    }
}

struct Idle;

#[async_trait]
impl Provider for Idle {}

#[tokio::test]
async fn a_guard_cannot_come_in_once_a_provider_is_there() {
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
            address: "a@x".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    repo.register_provider(account.id, Arc::new(Idle));
    let guard_store = Arc::new(
        TursoStore::open_with(dir.path().join("g.db"), encrypted())
            .await
            .unwrap(),
    );
    let clock = Arc::new(common::TestClock::default());
    let (engine, _) = engine_on(&guard_store, &clock, &dir.path().join("stop"));
    assert!(matches!(
        engine.attach(&repo),
        Err(GuardError::AttachRefused(_))
    ));
    assert!(!repo.capabilities().is_supported("toolcall.decide"));
}

#[tokio::test]
async fn attaching_to_a_second_messenger_changes_neither() {
    let w = World::open().await;
    let store = Arc::new(
        TursoStore::open_with(w.dir.path().join("m2.db"), encrypted())
            .await
            .unwrap(),
    );
    let second = MessengerRepo::open_turso(store, MessengerConfig::new(OWNER))
        .await
        .unwrap();
    assert!(matches!(
        w.engine.attach(&second),
        Err(GuardError::AttachRefused(_))
    ));
    assert!(
        !second.capabilities().is_supported("toolcall.decide"),
        "no half install"
    );
}

#[tokio::test]
async fn one_engine_per_store_and_per_repository() {
    let w = World::open().await;
    assert!(
        TursoGuardRepository::open(w.store.clone()).is_err(),
        "a second on this store"
    );
    let store = Arc::new(
        TursoStore::open_with(w.dir.path().join("g2.db"), encrypted())
            .await
            .unwrap(),
    );
    let repo = Arc::new(TursoGuardRepository::open(store).unwrap());
    let config = || fs_messenger_guard::GuardConfig::new(OWNER, w.dir.path().join("g2.stopped"));
    assert!(fs_messenger_guard::GuardEngine::open(config(), repo.clone()).is_ok());
    assert!(fs_messenger_guard::GuardEngine::open(config(), repo).is_err());
}
