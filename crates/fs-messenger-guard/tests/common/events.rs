//! Listening for events, and checking that a refused decision changes nothing.
#![allow(dead_code)]
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fs_messenger_core::{
    DecisionProblem as P, Error, Event, EventFilter, Listener, MessageAction, Messenger,
    Subscription, ToolCall, ToolCallDecision,
};
use fs_messenger_guard::Question;
use fs_objects::{ActivityFilter, Store};
use tokio::sync::mpsc;

use super::World;

pub struct Events(pub mpsc::UnboundedSender<Event>);

#[async_trait]
impl Listener for Events {
    async fn event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

pub async fn listen(
    w: &World,
    filter: EventFilter,
) -> (Subscription, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let subscription = w
        .repo
        .subscribe(filter, Arc::new(Events(tx)))
        .await
        .unwrap();
    (subscription, rx)
}

/// Whatever is still on its way. Everything before this call has been delivered or is about to be.
pub async fn drain(rx: &mut mpsc::UnboundedReceiver<Event>) {
    while tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .is_ok()
    {}
}

pub async fn next_call(rx: &mut mpsc::UnboundedReceiver<Event>, id: &str) -> ToolCall {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a toolCall event in time")
            .expect("the channel is open");
        if let Event::ToolCall(change) = event {
            if change.call.id == id {
                return change.call;
            }
        }
    }
}

pub async fn silence(rx: &mut mpsc::UnboundedReceiver<Event>) {
    let got = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(got.is_err(), "expected no event, got {got:?}");
}

/// Refused with this problem, and nothing written anywhere: state, shown call, log, either store.
pub async fn refused_as(w: &World, q: &Question, d: ToolCallDecision, problem: P) {
    let record = w.record().await;
    let shown = w.shown(q).await;
    let everything = ActivityFilter::default();
    let guard_log = w.store.activities(&everything).await.unwrap().len();
    let message_log = w.messages.activities(&everything).await.unwrap().len();
    let result = w
        .repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await;
    assert!(
        matches!(&result, Err(Error::DecisionRefused { problem: got, .. }) if *got == problem),
        "expected {problem:?}, got {result:?}"
    );
    assert_eq!(w.record().await, record, "the state changed");
    assert_eq!(w.shown(q).await, shown, "what is stored as shown changed");
    assert_eq!(
        w.store.activities(&everything).await.unwrap().len(),
        guard_log
    );
    assert_eq!(
        w.messages.activities(&everything).await.unwrap().len(),
        message_log
    );
}
