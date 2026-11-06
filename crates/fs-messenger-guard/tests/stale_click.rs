//! The stale click: what a person signed for cannot approve what replaced it. Missing input. Retries.

mod common;

use common::{AI, World};
use fs_messenger_core::{Error, MessageAction, Messenger, ToolCallState};
use fs_messenger_guard::{Allowance, Arguments, Caller, Gate, Refusal, Verdict};
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
