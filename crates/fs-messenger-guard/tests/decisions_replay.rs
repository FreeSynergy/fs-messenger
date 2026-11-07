//! `decide`: expiry, replay, and ids an agent picks.

mod common;

use common::{OWNER, World, args};
use fs_messenger_core::{
    Error, MessageAction, Messenger, ToolCallChoice, ToolCallDecision, ToolCallOutcome,
};
use fs_messenger_guard::{Allowance, Method, Question, Verdict};
use serde_json::json;
async fn send(w: &World, q: &Question, d: ToolCallDecision) -> fs_messenger_core::Result<()> {
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
}

fn good(w: &World, q: &Question) -> ToolCallDecision {
    w.owner
        .decide(&w.identity, q.message, &q.call, "allow-once")
}

/// Refused, with this word in the text, and the record the same to the byte.
async fn refused(w: &World, q: &Question, d: ToolCallDecision, why: &str) {
    let before = w.record().await;
    let log_before = w.engine.log().await.unwrap().len();
    let shown_before = w.shown(q).await;
    let result = send(w, q, d).await;
    let Err(Error::DecisionRefused { detail: text, .. }) = result else {
        panic!("{why}: expected a refusal, got {result:?}");
    };
    assert!(text.contains(why), "expected {why:?}, got {text:?}");
    assert_eq!(w.record().await, before, "{why}: the state changed");
    assert_eq!(
        w.engine.log().await.unwrap().len(),
        log_before,
        "{why}: logged"
    );
    assert_eq!(
        w.shown(q).await,
        shown_before,
        "{why}: what is shown changed"
    );
}

#[tokio::test]
async fn an_expired_question_is_refused() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    w.clock.advance(chrono::Duration::minutes(11));
    refused(&w, &q, good(&w, &q), "expired").await;
}

#[tokio::test]
async fn an_unused_allow_once_expires_too() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    send(&w, &q, good(&w, &q)).await.unwrap();
    w.clock.advance(chrono::Duration::minutes(3));
    let again = w.ask("message.delete", args("/m")).await;
    assert_ne!(
        again.call.id, q.call.id,
        "an expired approval is a new question"
    );
    let old = w.shown(&q).await;
    assert_eq!(old.outcome, ToolCallOutcome::Cancelled);
}

#[tokio::test]
async fn a_replayed_proof_is_refused_by_the_used_list_even_if_the_record_is_turned_back() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = good(&w, &q);
    send(&w, &q, d.clone()).await.unwrap();
    // Revision and stage alone would let this through. The used list does not.
    let w = w.restart().await;
    w.tamper(|doc| {
        doc["calls"][0]["stage"] = json!("question");
        doc["calls"][0]["revision"] = json!(1);
        doc["calls"][0]["expiresAt"] = json!("2999-01-01T00:00:00Z");
    })
    .await;
    let before = w.record().await;
    let result = send(&w, &q, d).await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("used before")),
        "{result:?}"
    );
    assert_eq!(w.record().await, before);
}

#[tokio::test]
async fn another_signature_for_the_same_bytes_is_the_same_proof() {
    use ed25519_compact::Noise;
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = good(&w, &q);
    send(&w, &q, d.clone()).await.unwrap();
    w.tamper(|doc| {
        doc["calls"][0]["stage"] = json!("question");
        doc["calls"][0]["revision"] = json!(1);
        doc["calls"][0]["expiresAt"] = json!("2999-01-01T00:00:00Z");
    })
    .await;
    // A second, valid, different signature over the very same bytes.
    let (message, revision) = (q.message.to_string(), q.call.revision.to_be_bytes());
    let arguments = fs_messenger_guard::decide_arguments(
        &message,
        &q.call.id,
        "allow-once",
        &q.call.arguments_digest,
        &revision,
    );
    let bytes =
        fs_messenger_guard::signed_bytes(&w.identity, Method::Decide, OWNER, &arguments, &[])
            .unwrap();
    let other = w
        .owner
        .pair
        .sk
        .sign(&bytes, Some(Noise::from_slice(&[3; 16]).unwrap()));
    assert_ne!(other.to_vec(), d.proof);
    let mut again = d;
    again.proof = other.to_vec();
    let result = send(&w, &q, again).await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("used before")),
        "{result:?}"
    );
}

#[tokio::test]
async fn an_id_chosen_by_an_agent_identifies_nothing() {
    use fs_messenger_core::{Ingest, ToolCall, ToolCallKind};
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    // An agent reports a call on the question's message, naming the question's id.
    let report = ToolCall {
        id: q.call.id.clone(),
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        ..ToolCall::default()
    };
    let refused_report = w.repo.record_tool_call(q.message, &q.call.id, report).await;
    assert!(
        matches!(refused_report, Err(Error::GuardRefused(_))),
        "{refused_report:?}"
    );
    assert_eq!(w.shown(&q).await, q.call, "the question is untouched");
    // A decision about an id only an agent knows is about no call the guard knows.
    let mut named = q.call.clone();
    named.id = "agent-chosen".into();
    let d = w.owner.decide(&w.identity, q.message, &named, "allow-once");
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("no such call"))
    );
}

#[test]
fn the_choices_stay_as_the_contract_has_them() {
    // There is deliberately no "always" that an agent could remember.
    let names: Vec<_> = [
        ToolCallChoice::Refuse,
        ToolCallChoice::AllowOnce,
        ToolCallChoice::AllowClass,
    ]
    .iter()
    .map(|c| serde_json::to_string(c).unwrap())
    .collect();
    assert_eq!(names, ["\"refuse\"", "\"allowOnce\"", "\"allowClass\""]);
}

#[tokio::test]
async fn an_answered_or_running_call_takes_no_second_answer_even_at_its_current_revision() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    send(&w, &q, good(&w, &q)).await.unwrap();
    // Approved: well signed, for the revision on screen. No question stands.
    let approved = w.shown(&q).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &approved, "allow-once");
    refused(&w, &q, d, "revision does not match").await;

    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(Allowance::Once { .. })
    ));
    // Running: a second answer would give it a second use.
    let running = w.shown(&q).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &running, "allow-once");
    refused(&w, &q, d, "revision does not match").await;
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Ask(_)
    ));
}
