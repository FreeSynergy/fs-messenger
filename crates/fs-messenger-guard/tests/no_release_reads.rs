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

/// A call that names no object counts as marked (strategy 4d(2), 2026-11-07): even with `readsOnly`
/// released it asks and never allows. Before, it allowed (F2 of the review of 9ea2830). Whoever wants
/// `Allow` for a read names the object (`check_object`, `tests/sensitive_reads.rs`).
#[tokio::test]
async fn a_released_reads_only_class_still_asks_for_a_read_that_names_no_object() {
    let w = World::with_ai().await;
    w.release(AI, fs_messenger_core::ToolCallRisk::ReadsOnly, "readsOnly")
        .await;
    let input = Arguments::from_text(r#"{"path":"/"}"#).unwrap();
    let verdict = w
        .engine
        .check(&Caller::new(AI), "message.open", &input)
        .await;
    assert!(matches!(verdict, Verdict::Ask(_)), "{verdict:?}");
}
