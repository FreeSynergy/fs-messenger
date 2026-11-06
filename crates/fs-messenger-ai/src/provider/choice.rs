//! [`RemoteChoice`]: a person chose to send this text to a model that leaves the device.
//!
//! Pattern: Command, used once. It names the exact text, so it cannot be carried over to another message,
//! and it runs out. Only the program builds one, from what a person did after seeing `data_path`. Nothing
//! that comes from a message or a model can reach this type, and a message saying "I agree" is text.
//! Within one process this is a rule and no wall; `DESIGN.md` says so.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

const LIFETIME: Duration = Duration::from_secs(120);
const HELD: usize = 16;

/// What a person did: this text may go out to this provider's remote model, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteChoice {
    person: String,
    text: String,
}

impl RemoteChoice {
    /// `person` chose to send exactly `text`. A frontend builds this only from a person's own action.
    pub fn new(person: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            person: person.into(),
            text: text.into(),
        }
    }

    /// Who chose.
    pub fn person(&self) -> &str {
        &self.person
    }
}

/// The choices not used yet.
#[derive(Default)]
pub(super) struct Choices {
    held: Mutex<VecDeque<(Instant, RemoteChoice)>>,
}

impl Choices {
    pub(super) fn add(&self, choice: RemoteChoice) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if held.len() >= HELD {
            held.pop_front();
        }
        held.push_back((Instant::now(), choice));
    }

    /// Whether someone chose this text, using that choice up.
    pub(super) fn take(&self, text: &str) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(at, _)| at.elapsed() < LIFETIME);
        match held.iter().position(|(_, c)| c.text == text) {
            Some(at) => {
                held.remove(at);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn once_for_the_exact_text_and_not_for_ever() {
        let choices = Choices::default();
        assert!(!choices.take("hello"));
        choices.add(RemoteChoice::new("me", "hello"));
        assert!(!choices.take("hello "), "another text");
        assert!(choices.take("hello"));
        assert!(!choices.take("hello"), "used up");
        choices.add(RemoteChoice::new("me", "later"));
        tokio::time::advance(LIFETIME).await;
        assert!(!choices.take("later"), "run out");
    }
}
