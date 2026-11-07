//! P2 of the review of 71541b3: a question or an approval that a version before the release of a program
//! left behind (v0.2.0) is no release. The release is looked at again when a decision is made and when
//! an approval is used up; questions from before the update do not count.

mod common;

use common::{World, args};
use fs_messenger_core::{Error, MessageAction, Messenger as _};
use fs_messenger_guard::{Allowance, Refusal, Verdict};
use serde_json::json;

/// What v0.2.0 left: the call, and no grant that names the program.
async fn as_before_the_update(w: &World) {
    w.tamper(|record| record["grants"] = json!([])).await;
}

#[tokio::test]
async fn an_open_question_from_before_cannot_be_approved_without_a_release() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    as_before_the_update(&w).await;
    let before = w.record().await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(result, Err(Error::DecisionRefused { .. })),
        "{result:?}"
    );
    assert_eq!(w.record().await, before, "a refusal changes nothing");
}

#[tokio::test]
async fn an_approval_from_before_is_not_used_up_without_a_release() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    as_before_the_update(&w).await;
    assert_eq!(
        w.check("message.delete", args("/m")).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
    // The approval is not lost by that: with a release it still counts, within its two minutes.
    w.release(
        common::AI,
        fs_messenger_core::ToolCallRisk::Changes,
        "changes",
    )
    .await;
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(Allowance::Once { call }) if call == q.call.id
    ));
}
