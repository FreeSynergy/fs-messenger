//! A running subscription: store feed in, events out.
//!
//! Pattern: Observer: one task per listener, ended by dropping the `Subscription`. A listener that
//! falls behind misses nothing: on `Lagged` the task reads the store's log from where it stood, as
//! the feed asks, and then skips what the channel still repeats. Beside the feed runs a channel for
//! what no store write stands behind (a refused decision): it is in memory, and a listener that falls
//! behind there misses it and has to read the message.

use std::sync::Arc;

use chrono::{SubsecRound, Utc};
use fs_store::{Activity, ActivityFilter, ActivityId, Store};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

use super::feed::events_of;
use crate::listener::{Listener, Subscription};
use crate::model::{Event, EventFilter, Kind};

async fn deliver(activity: &Activity, filter: &EventFilter, listener: &dyn Listener) {
    for item in events_of(activity) {
        if item.passes(filter) {
            listener.event(item.event).await;
        }
    }
}

/// An announcement is about a tool call of the engine, in a chat `Note`.
fn passes(event: &Event, filter: &EventFilter) -> bool {
    let Event::ToolCall(change) = event else {
        return false;
    };
    (filter.conversations.is_empty() || filter.conversations.contains(&change.conversation))
        && (filter.kinds.is_empty() || filter.kinds.contains(&Kind::Chat))
}

/// The receiver exists before this returns, so nothing committed afterwards can be missed.
pub(super) fn start(
    store: Arc<dyn Store>,
    mut announcements: Receiver<Event>,
    filter: EventFilter,
    listener: Arc<dyn Listener>,
) -> Subscription {
    let started = Utc::now().trunc_subsecs(6);
    let mut feed = store.subscribe();
    let task = tokio::spawn(async move {
        let mut last: Option<ActivityId> = None;
        let mut announcing = true;
        loop {
            let received = tokio::select! {
                received = feed.recv() => received,
                said = announcements.recv(), if announcing => {
                    match said {
                        Ok(event) => {
                            if passes(&event, &filter) {
                                listener.event(event).await;
                            }
                        }
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => announcing = false,
                    }
                    continue;
                }
            };
            match received {
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
