//! A release names the participant, class, program and where the text goes; the device signs all of
//! it. `revoke` takes releases back, all or of one program. A stop empties them.

mod common;

use common::{AI, GOES_TO, PROGRAM, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{AiGrant, Guard, GuardError, Method, Refusal, Verdict};

const DENIED: Verdict = Verdict::Refuse(Refusal::NotReleased);

fn grant(program: &str, goes_to: &str) -> AiGrant {
    AiGrant {
        profile: AI.into(),
        risk: R::ReadsOnly,
        program: program.into(),
        goes_to: goes_to.into(),
    }
}

#[tokio::test]
async fn the_signature_covers_program_and_destination() {
    let w = World::with_ai().await;
    let signed = w.release_proof(AI, "readsOnly"); // message, this device only
    for other in [grant("account", GOES_TO), grant(PROGRAM, "to Anthropic")] {
        let refused = w.engine.release(&other, &signed).await;
        assert!(
            matches!(refused, Err(GuardError::Refused(Refusal::BadProof))),
            "{other:?}: {refused:?}"
        );
    }
    assert!(w.engine.state().await.grants.is_empty());
    w.engine
        .release(&grant(PROGRAM, GOES_TO), &signed)
        .await
        .unwrap();
    assert_eq!(w.engine.state().await.grants, [grant(PROGRAM, GOES_TO)]);
}

#[tokio::test]
async fn a_release_must_name_a_program_and_where_the_text_goes() {
    let w = World::with_ai().await;
    for (program, goes_to, what) in [("", GOES_TO, "program"), (PROGRAM, "", "goesTo")] {
        let proof = w.owner.proof(
            &w.identity,
            Method::Release,
            &[
                AI.as_bytes(),
                b"readsOnly",
                program.as_bytes(),
                goes_to.as_bytes(),
            ],
        );
        let refused = w.engine.release(&grant(program, goes_to), &proof).await;
        assert!(
            matches!(refused, Err(GuardError::Refused(Refusal::Empty(w))) if w == what),
            "{what}: {refused:?}"
        );
    }
    assert!(w.engine.state().await.grants.is_empty());
}

#[tokio::test]
async fn the_log_says_which_program_and_where_it_goes() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    let log = w.engine.log().await.unwrap();
    let entry = log.iter().find(|e| e.event == "release").unwrap();
    assert_eq!(
        (entry.program.as_str(), entry.goes_to.as_str()),
        (PROGRAM, GOES_TO)
    );
}

#[tokio::test]
async fn revoke_takes_every_program_and_a_program_revoke_only_its_own() {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    w.release_in(AI, R::ReadsOnly, "account", "this device only")
        .await
        .unwrap();
    let proof = w.owner.proof(
        &w.identity,
        Method::Revoke,
        &[AI.as_bytes(), PROGRAM.as_bytes()],
    );
    w.engine.revoke_program(AI, PROGRAM, &proof).await.unwrap();
    assert_eq!(w.check("message.open", args("/")).await, DENIED);
    assert!(matches!(
        w.check("account.list", args("/")).await,
        Verdict::Allow(_)
    ));
    let all = w
        .owner
        .proof(&w.identity, Method::Revoke, &[AI.as_bytes(), b""]);
    w.engine.revoke(AI, &all).await.unwrap();
    assert_eq!(w.check("account.list", args("/")).await, DENIED);
}

#[tokio::test]
async fn a_program_revoke_signed_for_another_program_is_refused() {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    let proof = w
        .owner
        .proof(&w.identity, Method::Revoke, &[AI.as_bytes(), b"account"]);
    let refused = w.engine.revoke_program(AI, PROGRAM, &proof).await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn a_program_revoke_cancels_only_that_programs_open_questions() {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    w.release_in(AI, R::ReadsOnly, "account", "this device only")
        .await
        .unwrap();
    let kept = w.ask("account.remove", args("/a")).await;
    let gone = w.ask("message.delete", args("/m")).await;
    let proof = w.owner.proof(
        &w.identity,
        Method::Revoke,
        &[AI.as_bytes(), PROGRAM.as_bytes()],
    );
    w.engine.revoke_program(AI, PROGRAM, &proof).await.unwrap();
    assert_eq!(
        w.shown(&gone).await.outcome,
        fs_messenger_core::ToolCallOutcome::Cancelled
    );
    assert_eq!(
        w.shown(&kept).await.outcome,
        fs_messenger_core::ToolCallOutcome::Open
    );
}

#[tokio::test]
async fn a_stop_empties_the_releases_and_a_reopen_does_not_bring_them_back() {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    w.engine.reopen(&w.reopen_proof()).await.unwrap();
    assert!(w.engine.state().await.grants.is_empty());
    assert_eq!(w.check("message.open", args("/")).await, DENIED);
}
