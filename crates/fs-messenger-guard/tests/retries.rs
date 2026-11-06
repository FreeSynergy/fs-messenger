//! The limit on retries of a bare `input_required`, on open questions, and the end of a call.

mod common;

use common::{AI, World, args};
use fs_messenger_core::{MessageAction, Messenger, ToolCallState};
use fs_messenger_guard::{Caller, Gate, Refusal, Verdict};
#[tokio::test]
async fn retries_of_one_question_have_a_limit_and_the_question_stays_for_the_person() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    // At once: refused, and nothing is written, however tight the loop.
    let before = w.record().await;
    for _ in 0..3 {
        assert_eq!(
            w.check("message.delete", args("/m")).await,
            Verdict::Refuse(Refusal::RetryTooSoon)
        );
    }
    assert_eq!(w.record().await, before);
    // Up to the limit of 60 in all, then none.
    w.clock.advance(chrono::Duration::seconds(3));
    let again = w.check("message.delete", args("/m")).await;
    assert!(
        matches!(again, Verdict::Ask(a) if a.call.id == q.call.id),
        "a pause is fine"
    );
    let mut last = Verdict::Refuse(Refusal::Stopped);
    for _ in 0..70 {
        w.clock.advance(chrono::Duration::seconds(3));
        last = w.check("message.delete", args("/m")).await;
    }
    assert_eq!(last, Verdict::Refuse(Refusal::RetriesExhausted));
    assert_eq!(w.shown(&q).await.state, ToolCallState::Question);
    // The person can still answer, and that goes through whatever the counter says.
    let fresh = w.shown(&q).await;
    let d = w.owner.decide(&w.identity, q.message, &fresh, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn the_same_question_is_not_put_twice_and_open_questions_are_limited() {
    let w = World::with_ai().await;
    let a = w.ask("message.delete", args("/same")).await;
    let b = w.ask("message.delete", args("/same")).await;
    assert_eq!(a.call.id, b.call.id);
    for n in 0..15 {
        w.ask("message.delete", args(&format!("/{n}"))).await;
    }
    let before = w.record().await;
    assert_eq!(
        w.check("message.delete", args("/one-too-many")).await,
        Verdict::Refuse(Refusal::TooManyQuestions)
    );
    assert_eq!(w.record().await, before);
    // Expired ones do not count, and are settled.
    w.clock.advance(chrono::Duration::minutes(11));
    let fresh = w.ask("message.delete", args("/after")).await;
    assert_ne!(fresh.call.id, a.call.id);
    assert_eq!(w.shown(&a).await.state, ToolCallState::Settled);
}

#[tokio::test]
async fn finished_needs_the_callers_running_call_and_settled_is_final() {
    use fs_messenger_core::ToolCallOutcome;
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let caller = Caller::new(AI);
    assert_eq!(
        w.engine
            .finished(&caller, &q.call.id, ToolCallOutcome::Done)
            .await,
        Err(Refusal::SettledCall),
        "a question has not run"
    );
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
    assert_eq!(
        w.engine
            .finished(&Caller::new("someone"), &q.call.id, ToolCallOutcome::Done)
            .await,
        Err(Refusal::UnknownCall)
    );
    assert_eq!(
        w.engine
            .finished(&caller, &q.call.id, ToolCallOutcome::Cancelled)
            .await,
        Err(Refusal::UnknownOption),
        "only done or failed"
    );
    w.engine
        .finished(&caller, &q.call.id, ToolCallOutcome::Done)
        .await
        .unwrap();
    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Done);
    assert_eq!(
        w.engine
            .finished(&caller, &q.call.id, ToolCallOutcome::Failed)
            .await,
        Err(Refusal::SettledCall)
    );
}
