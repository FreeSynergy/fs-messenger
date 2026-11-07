//! The log shows what happened, without secrets, and undo through a store takes nothing back.

mod common;

use common::{AI, OWNER, World, args};
use fs_messenger_core::{MessageAction, Messenger, ToolCallOutcome, ToolCallRisk, ToolCallState};
use fs_messenger_guard::{AiGrant, Guard, GuardMode, Method, Refusal, Verdict};
use fs_store::{ActivityFilter, ActivityKind, Store};

const ENGINE: &str = "urn:freesynergy:messenger:guard";

/// Undo and redo everything anybody could ask for, on both stores.
async fn undo_everything(w: &World) {
    for store in [&w.store, &w.messages] {
        for actor in [OWNER, ENGINE, AI, "someone"] {
            for _ in 0..3 {
                let _ = store.undo(actor, 100).await;
                let _ = store.redo(actor, 100).await;
                let _ = store.undo(actor, 100).await;
            }
        }
    }
}

#[tokio::test]
async fn the_log_shows_what_happened() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let q = w.ask("message.delete", args("/secret/path")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    let proof = w.owner.proof(&w.identity, Method::Revoke, &[AI.as_bytes()]);
    w.engine.revoke(AI, &proof).await.unwrap();
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();

    let log = w.engine.log().await.unwrap();
    let events: Vec<_> = log.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(
        events,
        [
            "enrol", "reopen", "invite", "release", "decide", "revoke", "stop", "reopen"
        ]
    );
    assert!(log.iter().all(|e| e.actor == OWNER));
    assert!(log.iter().all(|e| e.at.is_some()));
    let decide = &log[4];
    assert_eq!(decide.choice, "allowOnce");
    assert_eq!(decide.call, q.call.id);
    assert_eq!(decide.capability, "message.delete");
    assert_eq!(decide.profile, AI);
    assert_eq!(log[3].risk, Some(ToolCallRisk::ReadsOnly));

    // It is in the store's own activity log, as log-only entries that no undo stack holds.
    let activities = w
        .store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(activities.len(), log.len());
    assert!(activities.iter().all(|a| !a.kind.changes_state()));
    assert_eq!(activities[2].kind, ActivityKind::Invite);
    assert_eq!(w.store.undo_state(OWNER).await.unwrap().undoable, 0);
}

#[tokio::test]
async fn the_log_holds_no_secrets() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/secret/path")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let (proof_hex, digest_hex) = (hex(&d.proof), hex(&q.call.arguments_digest));
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    let stop = w.stop_proof();
    let (nonce_hex, stop_hex) = (hex(&stop.nonce), hex(&stop.proof));
    w.engine.stop(&stop).await.unwrap();

    let activities = w
        .store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    let text = serde_json::to_string(&activities).unwrap();
    for secret in [
        proof_hex.as_str(),
        digest_hex.as_str(),
        nonce_hex.as_str(),
        stop_hex.as_str(),
        "/secret/path",
        &hex(&w.owner.public()),
    ] {
        assert!(!text.contains(secret), "the log holds {secret}");
    }
    // Nor does the state keep the arguments: only their digest.
    assert!(
        !w.record()
            .await
            .unwrap()
            .to_string()
            .contains("/secret/path")
    );
}

#[tokio::test]
async fn refusals_write_nothing_to_the_log() {
    let w = World::with_ai().await;
    let before = w.engine.log().await.unwrap().len();
    let mut bad = w.stop_proof();
    bad.proof[0] ^= 1;
    assert!(w.engine.stop(&bad).await.is_err());
    assert_eq!(w.engine.log().await.unwrap().len(), before);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn undo_cannot_reopen_a_stop_also_after_a_restart() {
    let w = World::with_ai().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let before = w.record().await;
    undo_everything(&w).await;
    assert_eq!(
        w.record().await,
        before,
        "the state object is out of its reach"
    );
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    let w = w.restart().await;
    undo_everything(&w).await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
}

#[tokio::test]
async fn undo_cannot_restore_a_release() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let grant = AiGrant {
        profile: AI.into(),
        risk: ToolCallRisk::ReadsOnly,
        program: "message".into(),
        goes_to: "this device only".into(),
    };
    assert_eq!(w.engine.state().await.grants, [grant]);
    let proof = w.owner.proof(&w.identity, Method::Revoke, &[AI.as_bytes()]);
    w.engine.revoke(AI, &proof).await.unwrap();
    undo_everything(&w).await;
    assert!(w.engine.state().await.grants.is_empty());
    let w = w.restart().await;
    undo_everything(&w).await;
    assert!(w.engine.state().await.grants.is_empty());
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Ask(_)
    ));
}

#[tokio::test]
async fn undo_of_what_a_message_shows_does_not_bring_a_settled_call_back() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Cancelled);

    // Undo as the engine's own actor: what the message shows goes back to the question.
    let undone = w.messages.undo(ENGINE, 1).await.unwrap();
    assert_eq!(undone.len(), 1);
    assert_eq!(
        w.shown(&q).await.state,
        ToolCallState::Question,
        "shown, and only shown"
    );

    // A click on it still gets nowhere: the engine goes by its own record.
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(click))
        .await;
    assert!(
        matches!(&result, Err(fs_messenger_core::Error::DecisionRefused { detail: t, .. }) if t.contains("settled")),
        "{result:?}"
    );
    assert!(!matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn undo_cannot_bring_back_an_allow_once() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(_)
    ));
    undo_everything(&w).await;
    assert!(!matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn compacting_and_clearing_the_log_reopen_nothing() {
    let w = World::with_ai().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.store.compact().await.unwrap();
    w.store.clear_history().await.unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
}
