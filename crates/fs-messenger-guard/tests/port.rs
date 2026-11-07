//! The port into the messenger, on a real store: what `settled` means there, and what it keeps.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{AI, OWNER, encrypted};
use fs_messenger_core::{
    EngineGuard, Messenger, MessengerConfig, MessengerRepo, Result, SenderKind, SenderLookup,
    ToolCall, ToolCallDecision, ToolCallOutcome, ToolCallState,
};
use fs_objects::{ObjectId, TursoStore};

struct Nobody;

#[async_trait]
impl EngineGuard for Nobody {
    async fn decide(&self, _: ObjectId, _: ObjectId, _: ToolCallDecision) -> Result<()> {
        Ok(())
    }
    async fn stop_conversation(&self, _: ObjectId) -> Result<()> {
        Ok(())
    }
    async fn open_for_reports(&self, _: &str) -> bool {
        true
    }
}

#[async_trait]
impl SenderLookup for Nobody {
    async fn sender_kind(&self, _: &str) -> SenderKind {
        SenderKind::Unknown
    }
}

#[tokio::test]
async fn settled_is_final_for_the_engine_too_and_an_empty_detail_keeps_the_shown_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = TursoStore::open_with(dir.path().join("m.db"), encrypted())
        .await
        .unwrap();
    let repo = MessengerRepo::open_turso(Arc::new(store), MessengerConfig::new(OWNER))
        .await
        .unwrap();
    let nobody = Arc::new(Nobody);
    let port = repo.install_guard(nobody.clone(), nobody).unwrap();

    let call = ToolCall {
        id: "c1".into(),
        state: ToolCallState::Question,
        detail: "the whole command".into(),
        revision: 1,
        ..ToolCall::default()
    };
    let placed = port.place_question(None, AI, call.clone()).await.unwrap();
    let shown = |repo: MessengerRepo| async move {
        let m = repo
            .message(placed.conversation, placed.message)
            .await
            .unwrap();
        m.tool_calls[0].clone()
    };

    let mut working = call.clone();
    working.state = ToolCallState::Working;
    working.detail.clear();
    working.revision = 2;
    port.update_call(placed.message, working.clone())
        .await
        .unwrap();
    let now = shown(repo.clone()).await;
    assert_eq!(now.state, ToolCallState::Working);
    assert_eq!(now.detail, "the whole command");

    assert_eq!(port.settle_open_calls(None).await.unwrap().settled, 1);
    let end = shown(repo.clone()).await;
    assert_eq!(end.outcome, ToolCallOutcome::Cancelled);
    assert_eq!(end.revision, 3);

    working.revision = 4;
    port.update_call(placed.message, working).await.unwrap();
    assert_eq!(shown(repo.clone()).await, end, "settled stays");
    assert_eq!(
        port.settle_open_calls(None).await.unwrap().settled,
        0,
        "none left"
    );
}
