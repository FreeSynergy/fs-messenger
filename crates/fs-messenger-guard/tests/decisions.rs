//! `decide`: every refusal changes nothing, and only a good decision does something.

mod common;

use common::{AI, Device, FRIEND, OWNER, World, args};
use fs_messenger_core::{
    Error, MessageAction, Messenger, ToolCallChoice, ToolCallDecision, ToolCallOutcome,
    ToolCallRisk, ToolCallState,
};
use fs_messenger_guard::{AiGrant, Allowance, Guard, Method, Question, Verdict};
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
async fn allow_once_allows_exactly_these_arguments_once() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    assert_eq!(q.call.state, ToolCallState::Question);
    assert_eq!(q.call.asked_of, OWNER);
    send(&w, &q, good(&w, &q)).await.unwrap();
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(Allowance::Once { call }) if call == q.call.id
    ));
    assert!(
        matches!(w.check("message.delete", args("/m")).await, Verdict::Ask(_)),
        "once"
    );
    let shown = w.shown(&q).await;
    assert_eq!(shown.state, ToolCallState::Working);
    assert!(shown.options.is_empty());
}

#[tokio::test]
async fn allow_once_does_not_cover_other_arguments() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    send(&w, &q, good(&w, &q)).await.unwrap();
    assert!(matches!(
        w.check("message.delete", args("/other")).await,
        Verdict::Ask(_)
    ));
    assert!(matches!(
        w.check("message.forward", args("/m")).await,
        Verdict::Ask(_)
    ));
}

#[tokio::test]
async fn allow_class_releases_this_participant_and_this_class() {
    let w = World::with_ai().await;
    let q = w.ask("message.flag", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-class");
    send(&w, &q, d).await.unwrap();
    assert_eq!(
        w.engine.state().await.grants,
        [AiGrant {
            profile: AI.into(),
            risk: ToolCallRisk::Changes
        }]
    );
    assert_eq!(
        w.check("message.unread", args("/n")).await,
        Verdict::Allow(Allowance::Class(ToolCallRisk::Changes))
    );
}

#[tokio::test]
async fn refuse_settles_the_call() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w.owner.decide(&w.identity, q.message, &q.call, "refuse");
    send(&w, &q, d).await.unwrap();
    let shown = w.shown(&q).await;
    assert_eq!(shown.state, ToolCallState::Settled);
    assert_eq!(shown.outcome, ToolCallOutcome::Refused);
    assert!(w.engine.state().await.grants.is_empty());
}

#[tokio::test]
async fn empty_fields_are_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let mut d = good(&w, &q);
    d.arguments_digest.clear();
    refused(&w, &q, d, "empty: argumentsDigest").await;
    let mut d = good(&w, &q);
    d.proof.clear();
    refused(&w, &q, d, "empty: proof").await;
    let mut d = good(&w, &q);
    d.signer.clear();
    refused(&w, &q, d, "empty: signer").await;
    let mut d = good(&w, &q);
    d.option.clear();
    refused(&w, &q, d, "empty: option").await;
    refused(&w, &q, ToolCallDecision::default(), "empty").await;
}

#[tokio::test]
async fn an_empty_asked_of_is_refused_as_it_can_never_match() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    w.tamper(|doc| doc["calls"][0]["askedOf"] = json!("")).await;
    refused(&w, &q, good(&w, &q), "empty: askedOf").await;
}

#[tokio::test]
async fn an_unknown_option_and_one_not_on_offer_are_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-always");
    refused(&w, &q, d, "unknown option").await;
    // Well signed, and still not on offer for this class: the brake holds in `decide` too.
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-class");
    refused(&w, &q, d, "unknown option").await;
}

#[tokio::test]
async fn a_signer_other_than_the_person_asked_is_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    // A real person with a real key, who was not asked.
    let friend = Device::new(FRIEND, 2);
    let proof = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), hex_of(&friend.public()).as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &proof)
        .await
        .unwrap();
    let d = friend.decide(&w.identity, q.message, &q.call, "allow-once");
    refused(&w, &q, d, "not the person asked").await;
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn a_settled_call_is_refused_even_with_a_good_signature() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let first = w.owner.decide(&w.identity, q.message, &q.call, "refuse");
    send(&w, &q, first).await.unwrap();
    let mut fresh_call = w.shown(&q).await;
    fresh_call.options.clear();
    let d = w
        .owner
        .decide(&w.identity, q.message, &fresh_call, "allow-once");
    refused(&w, &q, d, "settled").await;
}

#[tokio::test]
async fn a_digest_that_is_not_the_calls_is_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let mut other = q.call.clone();
    other.arguments_digest = vec![9; 32];
    let d = w.owner.decide(&w.identity, q.message, &other, "allow-once");
    refused(&w, &q, d, "digest does not match").await;
}

#[tokio::test]
async fn a_revision_that_is_not_the_calls_is_refused_and_zero_never_matches() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let mut other = q.call.clone();
    other.revision = 7;
    let d = w.owner.decide(&w.identity, q.message, &other, "allow-once");
    refused(&w, &q, d, "revision does not match").await;
    w.tamper(|doc| doc["calls"][0]["revision"] = json!(0)).await;
    let mut zero = q.call.clone();
    zero.revision = 0;
    let d = w.owner.decide(&w.identity, q.message, &zero, "allow-once");
    refused(&w, &q, d, "revision does not match").await;
}

#[tokio::test]
async fn a_signature_over_something_else_is_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let mut d = good(&w, &q);
    d.proof[0] ^= 1;
    refused(&w, &q, d, "proof does not hold").await;
    // Signed for allow-once, sent as allow-class on a call that offers both.
    let q2 = w.ask("message.flag", args("/m")).await;
    let mut d = w.owner.decide(&w.identity, q2.message, &q2.call, "refuse");
    d.option = "allow-class".into();
    refused(&w, &q2, d, "proof does not hold").await;
}

#[tokio::test]
async fn a_good_proof_for_another_message_is_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let q2 = w.ask("message.delete", args("/n")).await;
    // The right call, at another message: the record knows where it stands.
    let d = w
        .owner
        .decide(&w.identity, q2.message, &q.call, "allow-once");
    let before = w.record().await;
    let result = w
        .repo
        .act(q2.conversation, q2.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("no such call"))
    );
    assert_eq!(w.record().await, before);
}

#[tokio::test]
async fn an_expired_question_is_refused() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    w.clock.advance(chrono::Duration::minutes(11));
    refused(&w, &q, good(&w, &q), "expired").await;
}

#[tokio::test]
async fn an_unused_allow_once_expires_too() {
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
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
    use fs_messenger_core::{Ingest, RawToolCall, ToolCall, ToolCallKind};
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    // An agent reports a call on the question's message, naming the question's id.
    let report = ToolCall {
        id: q.call.id.clone(),
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        ..ToolCall::default()
    };
    let stored = w
        .repo
        .record_tool_call(q.message, &q.call.id, report)
        .await
        .unwrap();
    assert_ne!(stored.id, q.call.id, "the engine's id stays the engine's");
    assert_eq!(w.shown(&q).await, q.call, "the question is untouched");
    let _ = RawToolCall {
        agent_ref: String::new(),
        call: ToolCall::default(),
    };
    // A decision for what the agent got back is about no call the guard knows.
    let mut named = stored.clone();
    named.arguments_digest = vec![1; 32];
    named.revision = 1;
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
