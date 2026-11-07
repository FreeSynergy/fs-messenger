//! F1 and F2 of the review of 9ea2830 (strategy 4d, 2026-11-07): `allow-class` is nowhere, and every
//! way in (`check`, `check_object`, `revise`, a repeat after a revoke) goes through the same rules.
//! `revise` can never make a verdict milder than `check` would have given.

mod common;

use common::program::{Fixed, object};
use common::{AI, World, args};
use fs_messenger_core::{Error, MessageAction, Messenger, ToolCallChoice, ToolCallRisk as R};
use fs_messenger_guard::{
    Allowance, Arguments, Caller, Gate, Guard, Method, Question, Refusal, Sensitivity, Verdict,
};

fn input(path: &str) -> Arguments {
    Arguments::from_value(&args(path)).unwrap()
}

async fn open_read_without_input(w: &World) -> Question {
    w.clock.advance(chrono::Duration::seconds(3));
    let verdict = w
        .engine
        .check(&Caller::new(AI), "message.open", &Arguments::none())
        .await;
    match verdict {
        Verdict::Ask(q) => *q,
        other => panic!("expected a question, got {other:?}"),
    }
}

async fn read_object(w: &World) -> Verdict {
    w.clock.advance(chrono::Duration::seconds(3));
    let arguments = input("/x");
    let caller = Caller::new(AI);
    w.engine
        .check_object(&caller, "message.open", &arguments, &object())
        .await
}

async fn revise(w: &World, q: &Question, path: &str) -> Verdict {
    w.clock.advance(chrono::Duration::seconds(3));
    w.engine
        .revise(&Caller::new(AI), &q.call.id, &input(path))
        .await
}

fn only_refuse_and_once(q: &Question) {
    let options: Vec<_> = q.call.options.iter().map(|o| o.choice).collect();
    assert_eq!(options, [ToolCallChoice::Refuse, ToolCallChoice::AllowOnce]);
}

/// The chain of the review: only `changes` is released; a read without input asks; `revise` gave it
/// input, and the question then offered `allow-class`, which released `readsOnly`.
#[tokio::test]
async fn revise_does_not_bring_back_allow_class() {
    let w = World::with_ai().await;
    w.release(AI, R::Changes, "changes").await;
    let q = open_read_without_input(&w).await;
    let Verdict::Ask(revised) = revise(&w, &q, "/secret-mail").await else {
        panic!("a revised question is still a question");
    };
    only_refuse_and_once(&revised);
    let d = w
        .owner
        .decide(&w.identity, revised.message, &revised.call, "allow-class");
    let refused = w
        .repo
        .act(
            revised.conversation,
            revised.message,
            MessageAction::Decide(d),
        )
        .await;
    assert!(
        matches!(refused, Err(Error::DecisionRefused { .. })),
        "{refused:?}"
    );
    assert!(
        w.engine
            .state()
            .await
            .grants
            .iter()
            .all(|g| g.risk == R::Changes)
    );
    // Nothing is released for reads, so a read of another path is not allowed.
    let after = w.check("message.open", args("/other")).await;
    assert!(!matches!(after, Verdict::Allow(_)), "{after:?}");
}

/// A call that was asked because it could be a marked read stays one when it gets input: it shows
/// as asking every time, not as `readsOnly`, and it is never allowed by the class.
#[tokio::test]
async fn revise_cannot_turn_an_ask_into_the_released_class() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    let q = open_read_without_input(&w).await;
    let verdict = revise(&w, &q, "/secret-mail").await;
    let Verdict::Ask(revised) = verdict else {
        panic!("revise must not allow: {verdict:?}");
    };
    assert_ne!(
        revised.call.risk,
        R::ReadsOnly,
        "a marked read is not shown as plain reading"
    );
    only_refuse_and_once(&revised);
}

/// `revise` asks the same questions as `check`: a record without the release no longer lets it pass.
#[tokio::test]
async fn revise_checks_the_release_like_check_does() {
    let w = World::with_ai().await;
    w.release(AI, R::Changes, "changes").await;
    let q = open_read_without_input(&w).await;
    w.tamper(|doc| doc["grants"] = serde_json::json!([])).await;
    assert_eq!(
        revise(&w, &q, "/x").await,
        Verdict::Refuse(Refusal::NotReleased)
    );
}

/// After a revoke a repeat is refused on every way in, and the open question is gone.
#[tokio::test]
async fn after_a_revoke_every_way_in_refuses() {
    let w = World::with_ai().await;
    w.release(AI, R::Changes, "changes").await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    let q = open_read_without_input(&w).await;
    let proof = w
        .owner
        .proof(&w.identity, Method::Revoke, &[AI.as_bytes(), b""]);
    w.engine.revoke(AI, &proof).await.unwrap();
    let denied = Verdict::Refuse(Refusal::NotReleased);
    assert!(matches!(revise(&w, &q, "/x").await, Verdict::Refuse(_)));
    assert_eq!(w.check("message.open", args("/x")).await, denied);
    let by_object = read_object(&w).await;
    assert_eq!(by_object, denied);
}

/// `check_object`: a marked or unknown object asks without `allow-class`; only an unmarked one is
/// allowed by the class, and only with the class released.
#[tokio::test]
async fn check_object_allows_only_an_unmarked_object_of_a_released_class() {
    let w = World::with_ai().await;
    w.release(AI, R::Changes, "changes").await;
    assert!(w.engine.set_sensitivity(Fixed::new(Sensitivity::Unmarked)));
    assert_eq!(read_object(&w).await, Verdict::Refuse(Refusal::NotReleased));
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    assert_eq!(
        read_object(&w).await,
        Verdict::Allow(Allowance::Class(R::ReadsOnly))
    );
}

/// `check` without an object counts as marked: it asks, and never allows (F2).
#[tokio::test]
async fn check_without_an_object_asks_even_with_the_class_released() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    let Verdict::Ask(q) = w.check("message.open", args("/x")).await else {
        panic!("a read that names no object asks");
    };
    only_refuse_and_once(&q);
    assert_ne!(q.call.risk, R::ReadsOnly);
    // `changes` is not a read: it is still allowed by its class without an object.
    w.release(AI, R::Changes, "changes").await;
    assert!(matches!(
        w.check("message.flag", args("/x")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn no_question_ever_offers_allow_class() {
    let w = World::working().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    for (tool, path) in [
        ("message.delete", "/a"),
        ("message.send", "/b"),
        ("what.is.this", "/c"),
        ("message.open", "/d"),
    ] {
        let q = w.ask(tool, args(path)).await;
        only_refuse_and_once(&q);
        let revised = revise(&w, &q, "/changed").await;
        let Verdict::Ask(revised) = revised else {
            panic!("{tool}: {revised:?}")
        };
        only_refuse_and_once(&revised);
    }
}
