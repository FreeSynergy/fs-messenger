//! The stale click: what a person signed for cannot approve what replaced it. Missing input. Retries.

mod common;

use common::{AI, World, args};
use fs_messenger_core::{Error, MessageAction, Messenger, ToolCallRisk, ToolCallState};
use fs_messenger_guard::{
    Allowance, Arguments, Caller, Gate, Guard, Refusal, Verdict, arguments_digest,
};
use serde_json::json;

fn rm_home() -> serde_json::Value {
    json!({ "command": "rm -rf ~" })
}

fn harmless() -> serde_json::Value {
    json!({ "command": "ls" })
}

#[tokio::test]
async fn a_click_signed_for_q1_cannot_approve_q2_that_replaced_it() {
    let w = World::with_ai().await;
    // q1 stands on screen, and the person goes for Allow.
    let q1 = w.ask("shell.run", harmless()).await;
    let click = w
        .owner
        .decide(&w.identity, q1.message, &q1.call, "allow-once");
    // q2 now stands at the same place in the interface.
    let q2 = w.ask("shell.run", rm_home()).await;
    assert_ne!(q1.call.id, q2.call.id);
    assert_ne!(q1.call.arguments_digest, q2.call.arguments_digest);

    // Whichever way the frontend sends that click at q2, it does not hold.
    let before = w.record().await;
    let mut at_q2 = click.clone();
    at_q2.tool_call.clone_from(&q2.call.id);
    let result = w
        .repo
        .act(q2.conversation, q2.message, MessageAction::Decide(at_q2))
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("digest")),
        "{result:?}"
    );
    let mut with_q2_digest = click.clone();
    with_q2_digest.tool_call.clone_from(&q2.call.id);
    with_q2_digest
        .arguments_digest
        .clone_from(&q2.call.arguments_digest);
    let result = w
        .repo
        .act(
            q2.conversation,
            q2.message,
            MessageAction::Decide(with_q2_digest),
        )
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("proof")),
        "{result:?}"
    );
    let result = w
        .repo
        .act(
            q2.conversation,
            q2.message,
            MessageAction::Decide(click.clone()),
        )
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("no such call")),
        "{result:?}"
    );
    assert_eq!(w.record().await, before);

    // Sent as it was signed, it approves q1 and only q1.
    w.repo
        .act(q1.conversation, q1.message, MessageAction::Decide(click))
        .await
        .unwrap();
    assert_eq!(w.shown(&q2).await.state, ToolCallState::Question);
    assert!(matches!(
        w.check("shell.run", rm_home()).await,
        Verdict::Refuse(Refusal::RetryTooSoon) | Verdict::Ask(_)
    ));
    assert!(matches!(
        w.check("shell.run", harmless()).await,
        Verdict::Allow(Allowance::Once { call }) if call == q1.call.id
    ));
}

#[tokio::test]
async fn the_same_call_with_a_new_revision_does_not_take_the_old_click() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", harmless()).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");

    let new_input = Arguments::from_value(&rm_home()).unwrap();
    let Verdict::Ask(new) = w
        .engine
        .revise(&Caller::new(AI), &q.call.id, &new_input)
        .await
    else {
        panic!()
    };
    assert_eq!(new.call.id, q.call.id, "the same call");
    assert_eq!(new.call.revision, q.call.revision + 1);
    assert_ne!(new.call.arguments_digest, q.call.arguments_digest);
    assert_eq!(w.shown(&q).await.detail, new_input.canonical().unwrap());

    let before = w.record().await;
    let result = w
        .repo
        .act(
            q.conversation,
            q.message,
            MessageAction::Decide(click.clone()),
        )
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("digest")),
        "{result:?}"
    );
    // With the new digest put in by whoever sends it, the revision and the signature still fail.
    let mut digest_fixed = click.clone();
    digest_fixed
        .arguments_digest
        .clone_from(&new.call.arguments_digest);
    let result = w
        .repo
        .act(
            q.conversation,
            q.message,
            MessageAction::Decide(digest_fixed.clone()),
        )
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("revision")),
        "{result:?}"
    );
    digest_fixed.revision = new.call.revision;
    let result = w
        .repo
        .act(
            q.conversation,
            q.message,
            MessageAction::Decide(digest_fixed),
        )
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("proof")),
        "{result:?}"
    );
    assert_eq!(w.record().await, before);
    assert!(matches!(
        w.check("shell.run", rm_home()).await,
        Verdict::Refuse(_) | Verdict::Ask(_)
    ));

    // Back to the arguments the person saw: the revision has moved on, so still no.
    let back = Arguments::from_value(&harmless()).unwrap();
    assert!(matches!(
        w.engine.revise(&Caller::new(AI), &q.call.id, &back).await,
        Verdict::Ask(_)
    ));
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(click))
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { detail: t, .. }) if t.contains("revision")),
        "{result:?}"
    );
}

#[tokio::test]
async fn revise_needs_the_callers_own_open_call() {
    let w = World::with_ai().await;
    w.invite(common::OTHER_AI).await;
    let q = w.ask("shell.run", harmless()).await;
    let input = Arguments::from_value(&rm_home()).unwrap();
    let other = w
        .engine
        .revise(&Caller::new(common::OTHER_AI), &q.call.id, &input)
        .await;
    assert_eq!(other, Verdict::Refuse(Refusal::UnknownCall));
    let same = Arguments::from_value(&harmless()).unwrap();
    let Verdict::Ask(unchanged) = w.engine.revise(&Caller::new(AI), &q.call.id, &same).await else {
        panic!()
    };
    assert_eq!(
        unchanged.call.revision, q.call.revision,
        "same arguments, no change"
    );
}

#[tokio::test]
async fn an_approval_lapses_when_the_input_changes_and_a_running_call_cannot_be_changed() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", harmless()).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(click))
        .await
        .unwrap();

    let input = Arguments::from_value(&rm_home()).unwrap();
    let Verdict::Ask(asked) = w.engine.revise(&Caller::new(AI), &q.call.id, &input).await else {
        panic!("an approval must not survive new input")
    };
    assert_eq!(asked.call.state, ToolCallState::Question);
    assert!(
        !matches!(w.check("shell.run", harmless()).await, Verdict::Allow(_)),
        "the old arguments lost their approval too"
    );
    let click = w
        .owner
        .decide(&w.identity, q.message, &asked.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(click))
        .await
        .unwrap();
    assert!(matches!(
        w.check("shell.run", rm_home()).await,
        Verdict::Allow(_)
    ));
    let late = w
        .engine
        .revise(&Caller::new(AI), &q.call.id, &Arguments::none())
        .await;
    assert_eq!(late, Verdict::Refuse(Refusal::RunningCall));
}

#[tokio::test]
async fn without_arguments_the_digest_is_still_there_risk_unknown_and_allow_once_only() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let none = Arguments::none();
    let call = |args: &Arguments| {
        let (e, a) = (w.engine.clone(), args.clone());
        async move { e.check(&Caller::new(AI), "message.open", &a).await }
    };
    // Released for this class, and still: no input, no release counts.
    let Verdict::Ask(q) = call(&none).await else {
        panic!()
    };
    assert_eq!(q.call.risk, ToolCallRisk::Unknown);
    assert_eq!(
        q.call.arguments_digest,
        arguments_digest(AI, "message.open", &none)
    );
    assert!(!q.call.arguments_digest.is_empty());
    let ids: Vec<_> = q.call.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["refuse", "allow-once"]);
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-class");
    assert!(
        w.repo
            .act(q.conversation, q.message, MessageAction::Decide(d))
            .await
            .is_err()
    );
    assert_eq!(w.engine.state().await.grants.len(), 1, "nothing was added");
}

#[tokio::test]
async fn later_input_changes_the_digest_so_a_release_lapses() {
    let w = World::with_ai().await;
    let none = Arguments::none();
    let Verdict::Ask(q) = w
        .engine
        .check(&Caller::new(AI), "message.delete", &none)
        .await
    else {
        panic!()
    };
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    // The input arrives after the approval: not what the person approved.
    let input = Arguments::from_value(&args("/etc/passwd")).unwrap();
    assert!(matches!(
        w.engine
            .check(&Caller::new(AI), "message.delete", &input)
            .await,
        Verdict::Ask(_)
    ));
    // "No input" itself is what was approved, once.
    assert!(matches!(
        w.engine
            .check(&Caller::new(AI), "message.delete", &none)
            .await,
        Verdict::Allow(Allowance::Once { .. })
    ));
}

#[test]
fn no_input_and_an_empty_object_are_different_values() {
    let none = Arguments::none();
    let empty = Arguments::from_value(&json!({})).unwrap();
    assert_ne!(
        arguments_digest(AI, "t", &none),
        arguments_digest(AI, "t", &empty)
    );
    assert_eq!(
        Arguments::from_value(&serde_json::Value::Null).unwrap(),
        none
    );
    assert_eq!(Arguments::from_text("null").unwrap(), none);
}

#[test]
fn the_digest_covers_caller_tool_and_canonical_arguments_and_cannot_be_shifted() {
    let a = Arguments::from_text(r#"{"a":1,"b":[true,{"y":2,"x":3}]}"#).unwrap();
    let b = Arguments::from_text(r#"{ "b": [true, {"x":3, "y":2}], "a":1 }"#).unwrap();
    assert_eq!(a, b, "key order and white space do not count");
    assert_eq!(
        a.canonical().unwrap(),
        r#"{"a":1,"b":[true,{"x":3,"y":2}]}"#
    );
    let d = arguments_digest(AI, "t", &a);
    assert_eq!(d.len(), 32);
    assert_ne!(d, arguments_digest("other", "t", &a));
    assert_ne!(d, arguments_digest(AI, "other", &a));
    assert_ne!(
        d,
        arguments_digest(AI, "t", &Arguments::from_text("[1]").unwrap())
    );
    assert_ne!(
        arguments_digest("ab", "c", &a),
        arguments_digest("a", "bc", &a)
    );
}

#[test]
fn what_two_programs_could_read_differently_is_refused() {
    assert!(Arguments::from_text(r#"{"path":"/safe","path":"/"}"#).is_err());
    assert!(Arguments::from_text(r#"{"a":{"x":1,"x":2}}"#).is_err());
    assert!(Arguments::from_text(r#"[{"x":1,"x":2}]"#).is_err());
    assert!(Arguments::from_text("not json").is_err());
    let big = json!({ "x": "y".repeat(70 * 1024) });
    assert!(
        Arguments::from_value(&big).is_err(),
        "never cut off, so never over the limit"
    );
    let mut deep = json!(1);
    for _ in 0..80 {
        deep = json!([deep]);
    }
    assert!(Arguments::from_value(&deep).is_err());
}

#[tokio::test]
async fn what_is_shown_is_the_whole_input_and_cannot_hide_text() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", json!({ "c": "ok\u{202e}fr- mr" })).await;
    assert!(q.call.detail.contains("\\u{202e}"), "{}", q.call.detail);
    assert!(!q.call.detail.contains('\u{202e}'));
    assert_eq!(w.shown(&q).await.detail, q.call.detail);
    assert_eq!(q.call.summary, "", "agent text does not go here");
}

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
