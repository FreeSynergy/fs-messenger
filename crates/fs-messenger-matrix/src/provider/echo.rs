//! [`SentEvents`]: what this provider has sent, so a copy that returns is not delivered as new.
//!
//! Pattern: Value object over a bounded set. The core recognises a copy by its receipt, but only once
//! the receipt is stored. Between `send` returning and that, a sync in another task would deliver the
//! copy as a second message (core's open point "echo race"). Sending and syncing exclude each other,
//! so an event in the stream was entered here first. After a restart the receipt is stored or the
//! send was never recorded, so being empty then is right.

use std::collections::{HashSet, VecDeque};

const REMEMBERED: usize = 4096;

/// The events sent by this process, newest last.
#[derive(Debug, Default)]
pub(super) struct SentEvents {
    order: VecDeque<String>,
    known: HashSet<String>,
}

impl SentEvents {
    pub(super) fn remember(&mut self, event: String) {
        if self.known.insert(event.clone()) {
            self.order.push_back(event);
        }
        if self.order.len() > REMEMBERED
            && let Some(old) = self.order.pop_front()
        {
            self.known.remove(&old);
        }
    }

    pub(super) fn contains(&self, event: &str) -> bool {
        self.known.contains(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_remembers_and_stays_bounded() {
        let mut sent = SentEvents::default();
        sent.remember("$first".into());
        assert!(sent.contains("$first"));
        for i in 0..REMEMBERED {
            sent.remember(format!("$e{i}"));
        }
        assert!(!sent.contains("$first"));
        assert_eq!(sent.order.len(), REMEMBERED);
    }
}
