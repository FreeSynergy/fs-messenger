//! Blocker: a record that says `open` with a broken field reads as stopped, and recovery works for it.

mod common;

use common::{AI, OWNER, World, args};
use fs_messenger_core::ToolCallRisk;
use fs_messenger_guard::{Guard, GuardMode, Refusal, Verdict};
use serde_json::{Value, json};

type Breaks = Box<dyn Fn(&mut Value)>;

fn cases() -> Vec<(&'static str, Breaks)> {
    vec![
        ("engine zz", Box::new(|d| d["engine"] = json!("zz"))),
        ("engine empty", Box::new(|d| d["engine"] = json!(""))),
        ("engine short", Box::new(|d| d["engine"] = json!("ab"))),
        (
            "engine 64 not hex",
            Box::new(|d| d["engine"] = json!("z".repeat(64))),
        ),
        ("no devices", Box::new(|d| d["devices"] = json!({}))),
        (
            "owner no keys",
            Box::new(|d| d["devices"][OWNER] = json!([])),
        ),
        (
            "key not hex",
            Box::new(|d| d["devices"][OWNER][0]["publicKey"] = json!("zz")),
        ),
        (
            "key low order",
            Box::new(|d| d["devices"][OWNER][0]["publicKey"] = json!("00".repeat(32))),
        ),
        (
            "other key type",
            Box::new(|d| d["devices"][OWNER][0]["keyType"] = json!("rsa")),
        ),
        (
            "mode as a map",
            Box::new(|d| d["mode"] = json!({ "open": null })),
        ),
        (
            "mode missing",
            Box::new(|d| {
                d.as_object_mut().unwrap().remove("mode");
            }),
        ),
        (
            "another owner",
            Box::new(|d| d["owner"] = json!("https://elsewhere.invalid/me")),
        ),
    ]
}

#[tokio::test]
async fn each_broken_field_reads_as_stopped_and_recovery_goes_on_from_it() {
    for (name, breaks) in cases() {
        let w = World::working().await;
        w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
        assert!(matches!(
            w.read("message.open", args("/")).await,
            Verdict::Allow(_)
        ));
        w.tamper(&*breaks).await;
        assert!(w.engine.damage().await.is_some(), "{name}: not damage");
        assert_eq!(w.engine.state().await.mode, GuardMode::Stopped, "{name}");
        assert_eq!(
            w.read("message.open", args("/")).await,
            Verdict::Refuse(Refusal::Stopped),
            "{name}"
        );
        w.enrolment
            .recover_damaged(&w.owner.public())
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(w.engine.damage().await.is_none(), "{name}");
        assert_eq!(w.engine.state().await.mode, GuardMode::Stopped, "{name}");
    }
}

#[tokio::test]
async fn an_object_of_another_type_under_the_id_can_be_recovered_from() {
    use fs_store::traits::UnloggedObjects;
    let w = World::open().await;
    let id: fs_store::ObjectId = fs_messenger_guard::STATE_ID.parse().unwrap();
    let mut object = w.store.read_unlogged(id).await.unwrap().unwrap();
    object.envelope.object_type = "Note".into();
    w.store.put_unlogged(object).await.unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    w.enrolment
        .recover_damaged(&w.owner.public())
        .await
        .unwrap();
    assert!(w.engine.damage().await.is_none());
}

#[tokio::test]
async fn a_program_started_with_another_owner_reads_the_state_as_damage() {
    use std::sync::Arc;
    let w = World::open().await;
    let World {
        dir,
        store,
        repo,
        engine,
        enrolment,
        clock,
        ..
    } = w;
    drop((repo, engine, enrolment));
    let path = dir.path().to_path_buf();
    let guard_repo = Arc::new(fs_messenger_guard::TursoGuardRepository::open(store).unwrap());
    let (engine, _enrolment) = fs_messenger_guard::GuardEngine::open_with(
        fs_messenger_guard::GuardConfig::new(
            "https://elsewhere.invalid/me",
            path.join("other.stopped"),
        ),
        guard_repo,
        Arc::new(fs_messenger_guard::Ed25519Verifier),
        Arc::new(fs_messenger_guard::OwnerOnly),
        clock,
    )
    .unwrap();
    assert!(engine.damage().await.is_some());
    assert_eq!(engine.state().await.mode, GuardMode::Stopped);
}
