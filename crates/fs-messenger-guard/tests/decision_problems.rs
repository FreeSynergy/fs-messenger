//! A refused decision answers one of the contract's named problems, changes nothing else, and on stale
//! and settled a `toolCall` event says how the call really stands.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::{AI, Device, FRIEND, World, args};
use fs_messenger_core::{
    DecisionProblem as P, Error, Event, EventFilter, Listener, MessageAction, Messenger,
    Subscription, ToolCall, ToolCallDecision, ToolCallOutcome, ToolCallState,
};
use fs_messenger_guard::{Arguments, Caller, Gate, Guard, Method, Question, Verdict};
use fs_store::{ActivityFilter, Store};
use tokio::sync::mpsc;

struct Events(mpsc::UnboundedSender<Event>);

#[async_trait]
impl Listener for Events {
    async fn event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

async fn listen(w: &World, filter: EventFilter) -> (Subscription, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let subscription = w
        .repo
        .subscribe(filter, Arc::new(Events(tx)))
        .await
        .unwrap();
    (subscription, rx)
}

/// Whatever is still on its way. Everything before this call has been delivered or is about to be.
async fn drain(rx: &mut mpsc::UnboundedReceiver<Event>) {
    while tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .is_ok()
    {}
}

async fn next_call(rx: &mut mpsc::UnboundedReceiver<Event>, id: &str) -> ToolCall {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a toolCall event in time")
            .expect("the channel is open");
        if let Event::ToolCall(change) = event {
            if change.call.id == id {
                return change.call;
            }
        }
    }
}

async fn silence(rx: &mut mpsc::UnboundedReceiver<Event>) {
    let got = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(got.is_err(), "expected no event, got {got:?}");
}

/// Refused with this problem, and nothing written anywhere: state, shown call, log, either store.
async fn refused_as(w: &World, q: &Question, d: ToolCallDecision, problem: P) {
    let record = w.record().await;
    let shown = w.shown(q).await;
    let everything = ActivityFilter::default();
    let guard_log = w.store.activities(&everything).await.unwrap().len();
    let message_log = w.messages.activities(&everything).await.unwrap().len();
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { problem: got, .. }) if *got == problem),
        "expected {problem:?}, got {result:?}"
    );
    assert_eq!(w.record().await, record, "the state changed");
    assert_eq!(w.shown(q).await, shown, "what is stored as shown changed");
    assert_eq!(
        w.store.activities(&everything).await.unwrap().len(),
        guard_log
    );
    assert_eq!(
        w.messages.activities(&everything).await.unwrap().len(),
        message_log
    );
}

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
async fn a_stale_digest_or_revision_answers_stale_and_the_fresh_question_follows() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", args("ls")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let input = Arguments::from_value(&args("rm -rf ~")).unwrap();
    let Verdict::Ask(new) = w.engine.revise(&Caller::new(AI), &q.call.id, &input).await else {
        panic!()
    };
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;

    refused_as(&w, &q, click.clone(), P::Stale).await;
    let fresh = next_call(&mut rx, &q.call.id).await;
    assert_eq!(fresh.revision, new.call.revision);
    assert_eq!(fresh.arguments_digest, new.call.arguments_digest);
    assert_eq!(fresh.state, ToolCallState::Question);
    assert_eq!(
        fresh.detail,
        input.canonical().unwrap(),
        "the card can show the new question"
    );
    assert_eq!(fresh.options, new.call.options);

    // The digest put right by whoever sends it: the revision is still stale.
    let mut digest_fixed = click;
    digest_fixed
        .arguments_digest
        .clone_from(&new.call.arguments_digest);
    refused_as(&w, &q, digest_fixed, P::Stale).await;
    assert_eq!(
        next_call(&mut rx, &q.call.id).await.revision,
        new.call.revision
    );
}

#[tokio::test]
async fn a_click_on_a_call_answered_already_is_stale_and_shows_it_working() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(
            q.conversation,
            q.message,
            MessageAction::Decide(click.clone()),
        )
        .await
        .unwrap();
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    refused_as(&w, &q, click, P::Stale).await;
    assert_eq!(
        next_call(&mut rx, &q.call.id).await.state,
        ToolCallState::Working
    );
}

#[tokio::test]
async fn a_settled_call_answers_settled_and_its_end_follows() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let refuse = w.owner.decide(&w.identity, q.message, &q.call, "refuse");
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(refuse))
        .await
        .unwrap();
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    refused_as(&w, &q, click, P::Settled).await;
    let end = next_call(&mut rx, &q.call.id).await;
    assert_eq!(end.state, ToolCallState::Settled);
    assert_eq!(end.outcome, ToolCallOutcome::Refused);
    assert!(end.options.is_empty());
}

#[tokio::test]
async fn after_the_stop_a_click_answers_settled_with_cancelled() {
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
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
    let (_s, mut rx) = listen(&w, EventFilter::default()).await;
    drain(&mut rx).await;
    let d = friend.decide(&w.identity, q.message, &q.call, "allow-once");
    refused_as(&w, &q, d, P::NotAskedOf).await;
    silence(&mut rx).await;
}

#[tokio::test]
async fn empty_unknown_bad_replayed_and_expired_answer_invalid_and_no_event_follows() {
    let w = World::with_ai().await;
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
    let w = World::with_ai().await;
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
        conversations: vec![fs_store::ObjectId::new()],
        ..EventFilter::default()
    };
    let (_s, mut rx) = listen(&w, filter).await;
    refused_as(&w, &q, click, P::Settled).await;
    silence(&mut rx).await;
}

#[tokio::test]
async fn an_ai_as_the_person_asked_cannot_decide_even_with_a_key() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let ai = Device::new(AI, 9);
    let key: String = ai.public().iter().map(|b| format!("{b:02x}")).collect();
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
