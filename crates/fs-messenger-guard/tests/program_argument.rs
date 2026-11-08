//! F5 and F6 of the review of 9ea2830: a program argument is not trimmed quietly. What was signed is
//! what is used. Only the really empty field means every program; whitespace is refused.

mod common;

use common::{AI, World};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{Guard, GuardError, Method, ObjectRef, Refusal};

async fn two_releases() -> World {
    let w = World::with_ai().await;
    w.release_default(R::ReadsOnly).await;
    w.release_in(AI, R::ReadsOnly, "account").await.unwrap();
    w
}

#[tokio::test]
async fn a_program_of_whitespace_revokes_nothing_and_is_an_error() {
    let w = two_releases().await;
    for program in [" ", "\t", "  ", " message", "message ", "\u{a0}"] {
        let proof = w.owner.proof(
            &w.identity,
            Method::Revoke,
            &[AI.as_bytes(), program.as_bytes()],
        );
        let refused = w.engine.revoke_program(AI, program, &proof).await;
        assert!(
            matches!(refused, Err(GuardError::Refused(Refusal::BadName))),
            "{program:?}: {refused:?}"
        );
        assert_eq!(w.engine.state().await.grants.len(), 2, "{program:?}");
    }
}

#[tokio::test]
async fn only_the_empty_field_means_every_program_and_it_is_signed_so() {
    let w = two_releases().await;
    let proof = w
        .owner
        .proof(&w.identity, Method::Revoke, &[AI.as_bytes(), b""]);
    w.engine.revoke_program(AI, "", &proof).await.unwrap();
    assert!(w.engine.state().await.grants.is_empty(), "expected empty");
}

#[tokio::test]
async fn a_release_for_a_program_with_spaces_around_it_is_refused_not_trimmed() {
    let w = World::with_ai().await;
    let refused = w.release_in(AI, R::ReadsOnly, " message ").await;
    assert!(
        matches!(refused, Err(GuardError::Refused(Refusal::BadName))),
        "{refused:?}"
    );
    assert!(w.engine.state().await.grants.is_empty(), "expected empty");
}

/// F6(a): valid on construction means valid after trimming.
#[test]
fn an_object_of_whitespace_is_no_object() {
    for (program, id) in [
        ("  ", "  "),
        ("message", "  "),
        ("  ", "id"),
        ("", "id"),
        ("message", ""),
    ] {
        assert!(ObjectRef::new(program, id).is_err(), "{program:?} {id:?}");
    }
    let object = ObjectRef::new(" Message ", " id ").unwrap();
    assert_eq!((object.program(), object.id()), ("message", "id"));
}
