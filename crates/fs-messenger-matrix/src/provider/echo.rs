//! [`SentEvents`]: what this provider has sent, so a copy that returns is not delivered as new.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). A
//! bounded set over a bounded set. The core recognises a copy by its receipt, but only once the
//! receipt is stored. Between `send` returning and that, a sync in another task would deliver the
//! copy as a second message (core's open point "echo race"). Sending and syncing exclude each
//! other, so an event in the stream was entered here first. After a restart the receipt is stored
//! or the send was never recorded, so being empty then is right.

use std::collections::{HashSet, VecDeque};

use crate::receive::EventAlias;

const REMEMBERED: usize = 4096;

/// The events sent by this process, newest last.
#[derive(Debug, Default)]
pub(super) struct SentEvents {
    order: VecDeque<EventAlias>,
    known: HashSet<EventAlias>,
}

impl SentEvents {
    pub(super) fn remember(&mut self, alias: EventAlias) {
        if self.known.insert(alias.clone()) {
            self.order.push_back(alias);
        }
        if self.order.len() > REMEMBERED
            && let Some(old) = self.order.pop_front()
        {
            self.known.remove(&old);
        }
    }

    pub(super) fn contains(&self, alias: &EventAlias) -> bool {
        self.known.contains(alias)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_remembers_and_stays_bounded() {
        let first = EventAlias::new("!r:x", "$first");
        let mut sent = SentEvents::default();
        sent.remember(first.clone());
        assert!(sent.contains(&first));
        for i in 0..REMEMBERED {
            sent.remember(EventAlias::new("!r:x", format!("$e{i}")));
        }
        assert!(!sent.contains(&first));
        assert_eq!(sent.order.len(), REMEMBERED);
    }

    #[test]
    fn the_same_event_id_in_another_room_is_not_our_echo() {
        let mut sent = SentEvents::default();
        sent.remember(EventAlias::new("!ours:x", "$e"));
        assert!(!sent.contains(&EventAlias::new("!theirs:x", "$e")));
    }
}
