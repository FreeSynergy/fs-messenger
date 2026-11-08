//! A refused decision answers one of the contract's named problems, changes nothing else, and on stale
//! and settled a `toolCall` event says how the call really stands.

mod common;

use common::events::{drain, listen, next_call, refused_as};

use common::{AI, World, args};
use fs_messenger_core::{
    DecisionProblem as P, EventFilter, MessageAction, Messenger, ToolCallOutcome, ToolCallState,
};
use fs_messenger_guard::{Arguments, Caller, Gate, Verdict};
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
    let w = World::working().await;
    let q = w.ask("shell.run", args("ls")).await;
    let click = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    let input = Arguments::from_value(&args("rm -rf ~")).unwrap();
    let Verdict::Ask(new) = w
        .pause()
        .engine
        .revise(&Caller::new(AI), &q.call.id, &input)
        .await
    else {
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
    let w = World::working().await;
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
    let w = World::working().await;
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
    assert!(end.options.is_empty(), "expected empty");
}
