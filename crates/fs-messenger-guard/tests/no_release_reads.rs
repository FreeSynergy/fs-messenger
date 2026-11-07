//! ADR 0054, "Ohne Befugnis nichts": an invited AI with no release asks a `readsOnly` call. What the
//! guard answers is written down here as it is, so `DESIGN.md` can say it honestly.

mod common;

use common::{AI, World};
use fs_messenger_guard::{Arguments, Caller, Gate, Verdict};

#[tokio::test]
async fn an_invited_ai_without_a_release_asks_before_a_read() {
    let w = World::with_ai().await;
    let input = Arguments::from_text(r#"{"path":"/"}"#).unwrap();
    let verdict = w
        .engine
        .check(&Caller::new(AI), "message.open", &input)
        .await;
    assert!(
        matches!(verdict, Verdict::Ask(_)),
        "nothing is allowed without a release, also not a read: {verdict:?}"
    );
}

/// The gap the other way, stated as it is today: `readsOnly` is released as a whole class, also for
/// `message.open`, which reads text that the person may have marked. ADR 0054 ("Lesen ist nicht
/// harmlos") wants a question each time for what is marked. Not built; see `DESIGN.md`.
#[tokio::test]
async fn a_released_reads_only_class_allows_message_open_without_a_question() {
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
