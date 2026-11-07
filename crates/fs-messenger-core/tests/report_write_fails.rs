//! A report that is `working` when it is written, and found shut right after, is cancelled by a
//! second write. If that write fails once, it is tried again; if it keeps failing, the caller is told
//! so and the report stays `working` in the store (a limit named in `DESIGN.md` of the guard).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use common::{account, raw};
use fs_messenger_core::{
    EngineGuard, Ingest, Ingested, Messenger, MessengerConfig, MessengerRepo, Result, SenderKind,
    SenderLookup, ToolCall, ToolCallDecision, ToolCallKind, ToolCallOutcome, ToolCallState,
};
use fs_store::{
    Activity, ActivityFilter, ActivityId, ActivityKind, CompactionReport, LogEntry, LogStats,
    ObjectId, Query, Retention, Store, StoredObject, Timestamp, TursoStore, UndoState,
};
use tokio::sync::broadcast;

/// The real store, except that a number of writes can be let through and then a number can fail.
struct Flaky {
    real: TursoStore,
    pass: AtomicUsize,
    fail: AtomicUsize,
}

#[async_trait]
impl Store for Flaky {
    async fn put(&self, object: StoredObject, actor: &str) -> fs_store::Result<Activity> {
        if self.pass.load(Ordering::SeqCst) > 0 {
            self.pass.fetch_sub(1, Ordering::SeqCst);
        } else if self.fail.load(Ordering::SeqCst) > 0 {
            self.fail.fetch_sub(1, Ordering::SeqCst);
            return Err(fs_store::Error::Backend("disk full".into()));
        }
        Store::put(&self.real, object, actor).await
    }
    async fn put_and_log(
        &self,
        o: StoredObject,
        a: &str,
        e: Vec<LogEntry>,
    ) -> fs_store::Result<Vec<Activity>> {
        Store::put_and_log(&self.real, o, a, e).await
    }
    async fn get(&self, id: ObjectId) -> fs_store::Result<Option<StoredObject>> {
        Store::get(&self.real, id).await
    }
    async fn delete(&self, id: ObjectId, actor: &str) -> fs_store::Result<Activity> {
        Store::delete(&self.real, id, actor).await
    }
    async fn query(&self, q: &Query) -> fs_store::Result<Vec<StoredObject>> {
        Store::query(&self.real, q).await
    }
    async fn count(&self, q: &Query) -> fs_store::Result<usize> {
        Store::count(&self.real, q).await
    }
    async fn log(
        &self,
        k: ActivityKind,
        o: ObjectId,
        a: &str,
        p: serde_json::Value,
    ) -> fs_store::Result<Activity> {
        Store::log(&self.real, k, o, a, p).await
    }
    fn subscribe(&self) -> broadcast::Receiver<Activity> {
        Store::subscribe(&self.real)
    }
    async fn activities(&self, f: &ActivityFilter) -> fs_store::Result<Vec<Activity>> {
        Store::activities(&self.real, f).await
    }
    async fn undo(&self, a: &str, n: usize) -> fs_store::Result<Vec<Activity>> {
        Store::undo(&self.real, a, n).await
    }
    async fn redo(&self, a: &str, n: usize) -> fs_store::Result<Vec<Activity>> {
        Store::redo(&self.real, a, n).await
    }
    async fn undo_state(&self, a: &str) -> fs_store::Result<UndoState> {
        Store::undo_state(&self.real, a).await
    }
    async fn retention(&self) -> fs_store::Result<Retention> {
        Store::retention(&self.real).await
    }
    async fn set_retention(&self, r: Retention) -> fs_store::Result<()> {
        Store::set_retention(&self.real, r).await
    }
    async fn compact_at(&self, t: Timestamp) -> fs_store::Result<CompactionReport> {
        Store::compact_at(&self.real, t).await
    }
    async fn compact(&self) -> fs_store::Result<CompactionReport> {
        Store::compact(&self.real).await
    }
    async fn clear_history(&self) -> fs_store::Result<CompactionReport> {
        Store::clear_history(&self.real).await
    }
    async fn mark_synced(&self, up_to: ActivityId) -> fs_store::Result<usize> {
        Store::mark_synced(&self.real, up_to).await
    }
    async fn log_stats(&self) -> fs_store::Result<LogStats> {
        Store::log_stats(&self.real).await
    }
}

/// Open for the first question, shut from then on; every sender is an AI.
struct StopsAfterAnswer(AtomicBool);

#[async_trait]
impl EngineGuard for StopsAfterAnswer {
    async fn decide(&self, _: ObjectId, _: ObjectId, _: ToolCallDecision) -> Result<()> {
        Ok(())
    }
    async fn stop_conversation(&self, _: ObjectId) -> Result<()> {
        Ok(())
    }
    async fn open_for_reports(&self, _: &str) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

#[async_trait]
impl SenderLookup for StopsAfterAnswer {
    async fn sender_kind(&self, _: &str) -> SenderKind {
        SenderKind::Ai
    }
}

fn working() -> ToolCall {
    ToolCall {
        name: "rm".into(),
        kind: ToolCallKind::RunsCommand,
        state: ToolCallState::Working,
        ..ToolCall::default()
    }
}

/// A stored AI message, then a stop between the question and the write of a report, and `failing`
/// writes that fail right after the first write of the report.
async fn report_with_failing_cancel(failing: usize) -> (Result<ToolCall>, ToolCall) {
    let store = Arc::new(Flaky {
        real: TursoStore::open_in_memory().await.unwrap(),
        pass: AtomicUsize::new(0),
        fail: AtomicUsize::new(0),
    });
    let repo = MessengerRepo::open(store.clone(), MessengerConfig::new(common::ME))
        .await
        .unwrap();
    let late = Arc::new(StopsAfterAnswer(AtomicBool::new(false)));
    repo.install_guard(late.clone(), late.clone()).unwrap();
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "hi")).await.unwrap() else {
        panic!()
    };
    late.0.store(true, Ordering::SeqCst);
    store.pass.store(1, Ordering::SeqCst);
    store.fail.store(failing, Ordering::SeqCst);
    let result = repo.record_tool_call(id, "r", working()).await;
    let conversation = repo.conversations(None, false, None).await.unwrap().items[0].id;
    let shown = repo.message(conversation, id).await.unwrap().tool_calls;
    (result, shown[0].clone())
}

#[tokio::test]
async fn a_cancel_that_fails_once_is_written_again() {
    let (result, shown) = report_with_failing_cancel(1).await;
    let stored = result.unwrap();
    assert_eq!(stored.state, ToolCallState::Settled);
    assert_eq!(shown.state, ToolCallState::Settled, "{shown:?}");
    assert_eq!(shown.outcome, ToolCallOutcome::Cancelled);
}

#[tokio::test]
async fn a_cancel_that_keeps_failing_is_reported_and_never_claimed() {
    let (result, shown) = report_with_failing_cancel(1000).await;
    assert!(result.is_err(), "the caller must be told: {result:?}");
    // The limit, stated: the store still shows what was written first. The guard is shut, so
    // nothing acts on it, and the next stop settles it.
    assert_eq!(shown.state, ToolCallState::Working);
}
