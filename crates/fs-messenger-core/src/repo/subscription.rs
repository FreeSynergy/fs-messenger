//! A running subscription: store feed in, events out.
//!
//! Pattern: Observer: one task per listener, ended by dropping the `Subscription`. A listener that
//! falls behind misses nothing: on `Lagged` the task reads the store's log from where it stood, as
//! the feed asks, and then skips what the channel still repeats.

use std::sync::Arc;

use chrono::{SubsecRound, Utc};
use fs_store::{Activity, ActivityFilter, ActivityId, Store};
use tokio::sync::broadcast::error::RecvError;

use super::feed::events_of;
use crate::listener::{Listener, Subscription};
use crate::model::EventFilter;

async fn deliver(activity: &Activity, filter: &EventFilter, listener: &dyn Listener) {
    for item in events_of(activity) {
        if item.passes(filter) {
            listener.event(item.event).await;
        }
    }
}

/// The receiver exists before this returns, so nothing committed afterwards can be missed.
pub(super) fn start(
    store: Arc<dyn Store>,
    filter: EventFilter,
    listener: Arc<dyn Listener>,
) -> Subscription {
    let started = Utc::now().trunc_subsecs(6);
    let mut feed = store.subscribe();
    let task = tokio::spawn(async move {
        let mut last: Option<ActivityId> = None;
        loop {
            match feed.recv().await {
                Ok(activity) => {
                    if last.is_none_or(|l| activity.id > l) {
                        last = Some(activity.id);
                        deliver(&activity, &filter, listener.as_ref()).await;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    let wanted = ActivityFilter {
                        after: last,
                        ..ActivityFilter::default()
                    };
                    let Ok(missed) = store.activities(&wanted).await else {
                        continue;
                    };
                    for activity in missed.iter().filter(|a| a.at >= started) {
                        last = Some(activity.id);
                        deliver(activity, &filter, listener.as_ref()).await;
                    }
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
    Subscription::new(task.abort_handle())
}
