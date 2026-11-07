//! ADR 0054, "Ohne Befugnis nichts": an invited AI with no release is refused, not asked. The person
//! gives a release of their own accord; the AI cannot flood them with questions.

mod common;

use common::{AI, World};
use fs_messenger_guard::{Arguments, Caller, Gate, Refusal, Verdict};

#[tokio::test]
async fn an_invited_ai_without_a_release_is_refused_a_read_not_asked() {
    let w = World::with_ai().await;
    let input = Arguments::from_text(r#"{"path":"/"}"#).unwrap();
    let verdict = w
        .engine
        .check(&Caller::new(AI), "message.open", &input)
        .await;
    assert_eq!(verdict, Verdict::Refuse(Refusal::NotReleased));
}

/// A call that names no object is not looked up: a released `readsOnly` class allows it. A read that
/// names the object goes through `check_object` (`tests/sensitive_reads.rs`).
#[tokio::test]
async fn a_released_reads_only_class_allows_a_read_that_names_no_object() {
    let w = World::with_ai().await;
    w.release(AI, fs_messenger_core::ToolCallRisk::ReadsOnly, "readsOnly")
        .await;
    let input = Arguments::from_text(r#"{"path":"/"}"#).unwrap();
    let verdict = w
        .engine
        .check(&Caller::new(AI), "message.open", &input)
        .await;
    assert!(matches!(verdict, Verdict::Allow(_)), "{verdict:?}");
}
