//! From store activities to the contract's events.
//!
//! Pattern: Adapter (pure): one committed activity, with its before and after images, becomes the
//! events a listener cares about. `updated` is for what a person would see change, so an alias
//! learned in the background stays silent.

use fs_store::{Activity, StoredObject};

use super::consts::NOTE;
use super::message_codec::message_from;
use crate::model::{Event, EventFilter, Kind, Message, StateChange, ToolCallChange};

/// An event, and what a filter looks at.
pub(super) struct FeedItem {
    pub(super) event: Event,
    context: Option<String>,
    kind: Kind,
}

impl FeedItem {
    pub(super) fn passes(&self, filter: &EventFilter) -> bool {
        let conversation_ok = filter.conversations.is_empty()
            || self
                .context
                .as_deref()
                .is_some_and(|c| filter.conversations.iter().any(|id| id.to_string() == c));
        conversation_ok && (filter.kinds.is_empty() || filter.kinds.contains(&self.kind))
    }
}

fn note(o: Option<StoredObject>) -> Option<Message> {
    o.filter(|o| o.envelope.object_type == NOTE)
        .and_then(|o| message_from(o).ok())
}

/// Whether anything but the delivery state, the tool calls and the store's clock changed.
fn shown_differently(before: &Message, after: &Message) -> bool {
    let mut a = before.summary.clone();
    a.state = after.summary.state;
    a.envelope.updated = after.summary.envelope.updated;
    a != after.summary
        || before.body_text != after.body_text
        || before.stored.body_blob != after.stored.body_blob
        || before.reactions != after.reactions
        || before.attachments != after.attachments
}

fn changes(before: &Message, after: &Message) -> Vec<Event> {
    let id = after.id();
    let mut events = vec![];
    if before.summary.state != after.summary.state {
        events.push(Event::State(StateChange {
            message: id,
            state: after.summary.state,
        }));
    }
    let conversation = after
        .summary
        .envelope
        .context
        .as_deref()
        .and_then(|c| c.parse().ok());
    if let Some(conversation) = conversation {
        for call in &after.tool_calls {
            if !before.tool_calls.contains(call) {
                events.push(Event::ToolCall(ToolCallChange {
                    message: id,
                    call: call.clone(),
                    conversation,
                }));
            }
        }
    }
    if shown_differently(before, after) {
        events.push(Event::Updated(after.summary.clone()));
    }
    events
}

/// The events of one activity. Log-only activities, and objects that are not messages, have none.
pub(super) fn events_of(activity: &Activity) -> Vec<FeedItem> {
    if !activity.kind.changes_state() {
        return vec![];
    }
    let Ok(set) = activity.change_set() else {
        return vec![];
    };
    let (before, after) = (note(set.before), note(set.after));
    let (events, seen) = match (before, after) {
        (None, Some(after)) => (vec![Event::Created(after.summary.clone())], after),
        (Some(before), None) => (vec![Event::Deleted(before.id())], before),
        (Some(before), Some(after)) => (changes(&before, &after), after),
        (None, None) => return vec![],
    };
    let context = seen.summary.envelope.context;
    let kind = seen.summary.kind;
    events
        .into_iter()
        .map(|event| FeedItem {
            event,
            context: context.clone(),
            kind,
        })
        .collect()
}
