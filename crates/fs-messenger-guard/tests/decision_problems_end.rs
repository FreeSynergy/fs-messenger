//! Decisions refused for how a call ended; those that get no event; who hears them.

mod common;

use common::events::{drain, listen, next_call, refused_as, silence};
use std::fmt::Write as _;

use common::{AI, Device, FRIEND, World, args};
use fs_messenger_core::{
    DecisionProblem as P, Error, EventFilter, MessageAction, Messenger, ToolCallOutcome,
    ToolCallState,
};
use fs_messenger_guard::{Guard, Method};
use fs_objects::Store;
#[test]
fn the_uris_are_the_contracts() {
    assert_eq!(
        P::Stale.type_uri(),
        "https://freesynergy.net/problems/decision-stale"
    );
    assert_eq!(
        P::Settled.type_uri(),
        "https://freesynergy.net/problems/decision-settled"
    );
    assert_eq!(
        P::NotAskedOf.type_uri(),
        "https://freesynergy.net/problems/decision-not-asked-of-you"
    );
    assert_eq!(
        P::Invalid.type_uri(),
        "https://freesynergy.net/problems/decision-invalid"
    );
}
#[tokio::test]
async fn after_the_stop_a_click_answers_settled_with_cancelled() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    refused_as(&w, &q, click, P::Settled).await;
    assert_eq!(
        next_call(&mut rx, &q.call.id).await.outcome,
        ToolCallOutcome::Cancelled
    );
}

#[tokio::test]
async fn the_event_tells_the_engines_truth_not_what_an_undo_put_back_on_screen() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    w.messages
        .undo("urn:freesynergy:messenger:guard", 1)
        .await
        .unwrap();
    assert_eq!(w.shown(&q).await.state, ToolCallState::Question);
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    refused_as(&w, &q, click, P::Settled).await;
    assert_eq!(
        next_call(&mut rx, &q.call.id).await.outcome,
        ToolCallOutcome::Cancelled
    );
}

#[tokio::test]
async fn a_signer_who_was_not_asked_answers_not_asked_and_no_event_follows() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let friend = Device::new(FRIEND, 2);
    let key: String = friend.public().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    let d = friend.decide(&w.identity, q.message, &q.call, "allow-once");
    refused_as(&w, &q, d, P::NotAskedOf).await;
    silence(&mut rx).await;
}

#[tokio::test]
async fn empty_unknown_bad_replayed_and_expired_answer_invalid_and_no_event_follows() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;

    let good = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let mut empty = good.clone();
    empty.proof.clear();
    refused_as(&w, &q, empty, P::Invalid).await;
    let mut no_digest = good.clone();
    no_digest.arguments_digest.clear();
    refused_as(&w, &q, no_digest, P::Invalid).await;
    let unknown = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-always");
    refused_as(&w, &q, unknown, P::Invalid).await;
    let mut bad = good.clone();
    bad.proof[0] ^= 1;
    refused_as(&w, &q, bad, P::Invalid).await;
    let mut no_call = good.clone();
    no_call.tool_call = "no such call".into();
    refused_as(&w, &q, no_call, P::Invalid).await;
    silence(&mut rx).await;

    w.repo
        .act(
            q.conversation,
            q.message,
            MessageAction::Decide(good.clone()),
        )
        .await
        .unwrap();
    w.tamper(|doc| {
        doc["calls"][0]["stage"] = serde_json::json!("question");
        doc["calls"][0]["revision"] = serde_json::json!(1);
    })
    .await;
    drain(&mut rx).await;
    refused_as(&w, &q, good.clone(), P::Invalid).await; // replayed
    silence(&mut rx).await;

    let q2 = w.ask("message.delete", args("/n")).await;
    drain(&mut rx).await;
    let late = w
        .owner
        .decide(&w.identity, q2.message, &q2.call, "allow-once");
    w.clock.advance(chrono::Duration::minutes(11));
    refused_as(&w, &q2, late, P::Invalid).await; // expired
    silence(&mut rx).await;
}

#[tokio::test]
async fn a_listener_filtered_to_another_conversation_hears_nothing() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let other = w.ask("message.forward", args("/n")).await;
    assert_eq!(
        q.conversation, other.conversation,
        "one guard conversation per participant"
    );
    let refuse = w.owner.decide(&w.identity, q.message, &q.call, "refuse");
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(refuse))
        .await
        .unwrap();
    let filter = EventFilter {
        conversations: vec![fs_objects::ObjectId::new()],
        ..EventFilter::default()
    };
    let (_s, mut rx) = listen(&w, filter).await;
    refused_as(&w, &q, click, P::Settled).await;
    silence(&mut rx).await;
}

#[tokio::test]
async fn an_ai_as_the_person_asked_cannot_decide_even_with_a_key() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let ai = Device::new(AI, 9);
    let key: String = ai.public().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    w.tamper(|doc| {
        doc["calls"][0]["askedOf"] = serde_json::json!(AI);
        doc["devices"][AI] = serde_json::json!([{ "keyType": "ed25519", "publicKey": key }]);
    })
    .await;
    let d = ai.decide(&w.identity, q.message, &q.call, "allow-once");
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { problem: P::Invalid, detail }) if detail.contains("marked as AI")),
        "{result:?}"
    );
}

#[tokio::test]
async fn stopped_refuses_a_good_decision_on_a_call_a_record_shows_as_open() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let good = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.tamper(|doc| {
        doc["calls"][0]["stage"] = serde_json::json!("question");
        doc["calls"][0]["revision"] = serde_json::json!(1);
        doc["calls"][0]["expiresAt"] = serde_json::json!("2999-01-01T00:00:00Z");
    })
    .await;
    refused_as(&w, &q, good, P::Invalid).await;
}
