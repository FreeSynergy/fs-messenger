//! A stalled guard store and the messenger; the last look before anything is allowed.

mod common;

use fs_messenger_core::Messenger as _;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::rigs::Rig;
use fs_messenger_core::{Ingest, SenderKind};
use fs_messenger_guard::{Guard, Method, Refusal, Verdict};
#[tokio::test]
async fn a_stalled_guard_store_does_not_stall_messenger_writes() {
    let r = Rig::open().await;
    let mail = r
        .repo
        .add_account(fs_messenger_core::AccountSetup {
            address: "me@x".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    r.faulty.hang_loads.store(true, Ordering::SeqCst);
    let mut raw = fs_messenger_core::RawMessage::new(
        fs_messenger_core::ExternalId::new("uid", "1"),
        fs_messenger_core::ExternalId::new("thread", "1"),
        fs_messenger_core::Kind::Chat,
        common::AI,
        "hello",
    );
    raw.encryption = fs_messenger_core::Encryption::Mls;
    let started = Instant::now();
    let stored = r.repo.ingest(mail.id, raw).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    let fs_messenger_core::Ingested::Stored(id) = stored else {
        panic!()
    };
    let conv = r.repo.conversations(None, false, None).await.unwrap().items[0].id;
    assert_eq!(
        r.repo.message(conv, id).await.unwrap().summary.sender_kind,
        SenderKind::Unknown,
        "no answer reads as not known"
    );
}

#[tokio::test]
async fn sender_lookup_needs_no_lock_of_the_engine_so_nothing_deadlocks() {
    // An operation of the engine holds its lock at a read that waits, and a message comes in.
    let r = Rig::open().await;
    let mail = r
        .repo
        .add_account(fs_messenger_core::AccountSetup {
            address: "me@x".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    let engine = r.engine.clone();
    // A read that names an unmarked object takes the class path; `check` would ask, and put a
    // question conversation beside the mail.
    let holding = tokio::spawn(async move {
        common::program::read_unmarked(&engine, "message.open", &serde_json::json!({"path": "/"}))
            .await
    });
    r.faulty.entered.notified().await;
    let mut raw = fs_messenger_core::RawMessage::new(
        fs_messenger_core::ExternalId::new("uid", "2"),
        fs_messenger_core::ExternalId::new("thread", "2"),
        fs_messenger_core::Kind::Chat,
        common::AI,
        "hello",
    );
    raw.encryption = fs_messenger_core::Encryption::Mls;
    let stored = tokio::time::timeout(Duration::from_secs(1), r.repo.ingest(mail.id, raw))
        .await
        .expect("no deadlock")
        .unwrap();
    r.faulty.go.notify_one();
    holding.await.unwrap();
    let fs_messenger_core::Ingested::Stored(id) = stored else {
        panic!()
    };
    let conv = r.repo.conversations(None, false, None).await.unwrap().items[0].id;
    assert_eq!(
        r.repo.message(conv, id).await.unwrap().summary.sender_kind,
        SenderKind::Ai
    );
}

#[tokio::test]
async fn the_last_look_before_a_class_allow() {
    let r = Rig::open().await;
    // A check has read an open state and waits. The stop comes in.
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    let engine = r.engine.clone();
    let checking = tokio::spawn(async move {
        common::program::read_unmarked(&engine, "message.open", &serde_json::json!({"path": "/"}))
            .await
    });
    r.faulty.entered.notified().await;
    let (engine, proof) = (r.engine.clone(), r.proof(Method::Stop));
    let stopping = tokio::spawn(async move { engine.stop(&proof).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    r.faulty.go.notify_one();
    assert_eq!(checking.await.unwrap(), Verdict::Refuse(Refusal::Stopped));
    stopping.await.unwrap().unwrap();
}

#[tokio::test]
async fn the_last_look_before_an_approval_is_used() {
    use fs_messenger_core::MessageAction;
    use fs_messenger_guard::{Arguments, Caller, Gate};
    let r = Rig::open().await;
    let input = Arguments::from_text(r#"{"path":"/m"}"#).unwrap();
    let caller = Caller::new(common::AI);
    let Verdict::Ask(q) = r.engine.check(&caller, "message.delete", &input).await else {
        panic!()
    };
    let d = r
        .owner
        .decide(&r.identity, q.message, &q.call, "allow-once");
    r.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    r.faulty.hold_load.store(true, Ordering::SeqCst);
    let (engine, moved) = (r.engine.clone(), input.clone());
    let checking =
        tokio::spawn(async move { engine.check(&caller, "message.delete", &moved).await });
    r.faulty.entered.notified().await;
    let (engine, proof) = (r.engine.clone(), r.proof(Method::Stop));
    let stopping = tokio::spawn(async move { engine.stop(&proof).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    r.faulty.go.notify_one();
    assert_eq!(checking.await.unwrap(), Verdict::Refuse(Refusal::Stopped));
    stopping.await.unwrap().unwrap();
}
