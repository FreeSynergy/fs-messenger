//! One operation's questions to the source of destinations.
//!
//! Pattern: Decorator over the Strategy [`DestinationLookup`], stretched (it keeps a memo): it
//! implements the same trait, asks the wrapped source at most once for a participant and a program,
//! and has one wait for everything the operation asks. A source that hangs costs that wait once, not once for
//! every call that is shown, and the engine's operations lock is held for no longer. It is made for one
//! operation and thrown away with it. The source runs in a task of its own; a panic or no answer in
//! time reads as [`Destination::Unknown`], and so does everything asked after the wait has run out.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::Instant;

use crate::destination::{Destination, DestinationLookup};

type Key = (String, String);

/// What one operation has asked of the source, and the time it may still take.
pub(super) struct OnceLookup {
    source: Arc<dyn DestinationLookup>,
    wait: Duration,
    ends: Mutex<Option<Instant>>,
    seen: Mutex<Vec<(Key, Destination)>>,
}

impl OnceLookup {
    /// A lookup over this source; `wait` is all it may take for the whole operation.
    pub(super) fn new(source: Arc<dyn DestinationLookup>, wait: Duration) -> Self {
        Self {
            source,
            wait,
            ends: Mutex::new(None),
            seen: Mutex::new(vec![]),
        }
    }

    fn remembered(&self, key: &Key) -> Option<Destination> {
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.iter().find(|(k, _)| k == key).map(|(_, d)| d.clone())
    }

    /// Time left of the one wait; it starts with the first question that goes to the source.
    fn left(&self) -> Duration {
        let mut ends = self.ends.lock().unwrap_or_else(PoisonError::into_inner);
        let ends = *ends.get_or_insert_with(|| Instant::now() + self.wait);
        ends.saturating_duration_since(Instant::now())
    }
}

#[async_trait]
impl DestinationLookup for OnceLookup {
    async fn destination(&self, participant: &str, program: &str) -> Destination {
        let key = (participant.to_string(), program.to_string());
        if let Some(found) = self.remembered(&key) {
            return found;
        }
        let source = self.source.clone();
        let (who, program) = key.clone();
        let mut task = tokio::spawn(async move { source.destination(&who, &program).await });
        let answer = match tokio::time::timeout(self.left(), &mut task).await {
            Ok(Ok(destination)) => destination,
            Ok(Err(_)) => Destination::Unknown,
            Err(_) => {
                task.abort();
                Destination::Unknown
            }
        };
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.push((key, answer.clone()));
        answer
    }
}
