//! ADR 0054, "Lesen ist nicht harmlos": what is marked asks at every read, cannot be released, and
//! a mark comes only from the source the engine asks, never from the call.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use common::program::{Fixed, Hangs, Panics, object};
use common::{AI, World};
use fs_messenger_core::{
    Error, MessageAction, Messenger, ToolCallChoice, ToolCallOutcome, ToolCallRisk as R,
};
use fs_messenger_guard::{
    Allowance, Arguments, Caller, Gate, Guard, NoMarks, Refusal, Sensitivity, SensitivityLookup,
    Verdict,
};
use serde_json::json;

async fn read(w: &World, input: serde_json::Value) -> Verdict {
    w.clock.advance(chrono::Duration::seconds(3));
    let arguments = Arguments::from_value(&input).unwrap();
    w.engine
        .check_object(&Caller::new(AI), "message.open", &arguments, &object())
        .await
}

async fn world_reading(source: Arc<dyn SensitivityLookup>) -> World {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    assert!(w.engine.set_sensitivity(source));
    w
}

fn question(verdict: Verdict) -> fs_messenger_guard::Question {
    match verdict {
        Verdict::Ask(q) => *q,
        other => panic!("expected a question, got {other:?}"),
    }
}

#[tokio::test]
async fn unmarked_is_the_released_class() {
    let w = world_reading(Fixed::new(Sensitivity::Unmarked)).await;
    assert_eq!(
        read(&w, json!({"id": "1"})).await,
        Verdict::Allow(Allowance::Class(R::ReadsOnly))
    );
}

#[tokio::test]
async fn marked_asks_every_time_also_with_the_class_released() {
    let w = world_reading(Fixed::new(Sensitivity::Marked)).await;
    for round in 0..3 {
        let q = question(read(&w, json!({"id": "1"})).await);
        // No "always": no allow-class option, and nothing the question shows says readsOnly.
        let options: Vec<_> = q.call.options.iter().map(|o| o.choice).collect();
        assert_eq!(
            options,
            [ToolCallChoice::Refuse, ToolCallChoice::AllowOnce],
            "round {round}"
        );
        assert_ne!(q.call.risk, R::ReadsOnly);
        let d = w
            .owner
            .decide(&w.identity, q.message, &q.call, "allow-once");
        w.repo
            .act(q.conversation, q.message, MessageAction::Decide(d))
            .await
            .unwrap();
        let used = read(&w, json!({"id": "1"})).await;
        assert!(
            matches!(used, Verdict::Allow(Allowance::Once { .. })),
            "{used:?}"
        );
        // Used up: the next read asks again.
    }
    assert!(
        w.engine
            .state()
            .await
            .grants
            .iter()
            .all(|g| g.risk == R::ReadsOnly)
    );
}

#[tokio::test]
async fn nobody_can_release_it_by_answering_allow_class() {
    let w = world_reading(Fixed::new(Sensitivity::Marked)).await;
    let q = question(read(&w, json!({"id": "1"})).await);
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-class");
    let refused = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(refused, Err(Error::DecisionRefused { .. })),
        "{refused:?}"
    );
}

#[tokio::test]
async fn unknown_asks_like_marked_and_so_does_the_first_source() {
    let w = world_reading(Fixed::new(Sensitivity::Unknown)).await;
    question(read(&w, json!({"id": "1"})).await);
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await; // no source set: NoMarks
    question(read(&w, json!({"id": "1"})).await);
    assert_eq!(NoMarks.sensitivity(&object()).await, Sensitivity::Unknown);
}

#[tokio::test]
async fn a_source_that_hangs_or_panics_reads_as_marked() {
    for source in [
        Arc::new(Hangs) as Arc<dyn SensitivityLookup>,
        Arc::new(Panics),
    ] {
        let w = world_reading(source).await;
        let started = Instant::now();
        question(read(&w, json!({"id": "1"})).await);
        assert!(started.elapsed().as_secs() < 5, "the wait has a limit");
    }
}

#[tokio::test]
async fn what_the_call_says_about_itself_changes_nothing() {
    let marked = Fixed::new(Sensitivity::Marked);
    let w = world_reading(marked.clone()).await;
    for claim in [
        json!({"id": "1", "sensitive": false}),
        json!({"id": "1", "marked": false, "readsOnly": true}),
        json!({"id": "1", "envelope": {"sensitive": false}}),
    ] {
        question(read(&w, claim).await);
    }
    assert!(marked.asked.load(Ordering::SeqCst) >= 3);
    let unmarked = Fixed::new(Sensitivity::Unmarked);
    let w = world_reading(unmarked).await;
    let v = read(&w, json!({"id": "1", "sensitive": true})).await;
    assert!(
        matches!(v, Verdict::Allow(_)),
        "a claim does not mark either: {v:?}"
    );
}

#[tokio::test]
async fn marked_without_any_release_in_the_program_is_refused_not_asked() {
    let w = World::with_ai().await;
    assert!(w.engine.set_sensitivity(Fixed::new(Sensitivity::Marked)));
    assert_eq!(
        read(&w, json!({"id": "1"})).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
}

#[tokio::test]
async fn no_answer_from_the_person_never_becomes_access() {
    let w = world_reading(Fixed::new(Sensitivity::Marked)).await;
    let q = question(read(&w, json!({"id": "1"})).await);
    w.clock.advance(chrono::Duration::minutes(11));
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let late = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(late, Err(Error::DecisionRefused { .. })),
        "{late:?}"
    );
    // The read asks again; it is never allowed by the time that passed.
    let again = question(read(&w, json!({"id": "1"})).await);
    assert_ne!(again.call.id, q.call.id);
    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Cancelled);
}

#[tokio::test]
async fn the_same_answer_twice_is_refused() {
    let w = world_reading(Fixed::new(Sensitivity::Marked)).await;
    let q = question(read(&w, json!({"id": "1"})).await);
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d.clone()))
        .await
        .unwrap();
    let again = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(again, Err(Error::DecisionRefused { .. })),
        "{again:?}"
    );
}

#[tokio::test]
async fn the_question_names_where_the_text_goes() {
    let w = World::with_ai().await;
    w.release_in(AI, R::ReadsOnly, "message", "to Anthropic")
        .await
        .unwrap();
    assert!(w.engine.set_sensitivity(Fixed::new(Sensitivity::Marked)));
    let q = question(read(&w, json!({"id": "1"})).await);
    assert!(
        q.call.summary.contains("to Anthropic"),
        "{:?}",
        q.call.summary
    );
    assert!(w.shown(&q).await.summary.contains("to Anthropic"));
}

#[tokio::test]
async fn a_stop_empties_releases_so_a_marked_read_is_refused_after_reopen() {
    let w = world_reading(Fixed::new(Sensitivity::Marked)).await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert_eq!(
        read(&w, json!({"id": "1"})).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
}
