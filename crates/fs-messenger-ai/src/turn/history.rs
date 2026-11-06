//! What a model hears of a conversation: the turns that ended well, in memory, within limits.
//!
//! Pattern: Repository (in memory). After a restart a conversation starts afresh, and a turn that failed
//! or was stopped leaves nothing, so the model is never told it said what it did not finish. Reasoning and
//! tool calls are not sent back: nothing answers a tool call yet.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use crate::limits::Limits;
use crate::wire::{ChatLine, Role};

/// By the conversation's alias.
pub(crate) struct History {
    lines: Mutex<HashMap<String, VecDeque<ChatLine>>>,
    max_lines: usize,
    max_bytes: usize,
}

impl History {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            lines: Mutex::new(HashMap::new()),
            max_lines: limits.history_messages,
            max_bytes: limits.history_bytes,
        }
    }

    /// The conversation so far, and `text` as the person's next line.
    pub(crate) fn with_next(&self, conversation: &str, text: &str) -> Vec<ChatLine> {
        let held = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        let mut lines: Vec<ChatLine> = held
            .get(conversation)
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();
        lines.push(ChatLine {
            role: Role::User,
            text: text.to_string(),
        });
        lines
    }

    /// A turn ended well. The oldest go first, a person's line with its answer.
    pub(crate) fn finished(&self, conversation: &str, asked: String, answer: String) {
        let mut held = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        let lines = held.entry(conversation.to_string()).or_default();
        lines.push_back(ChatLine {
            role: Role::User,
            text: asked,
        });
        lines.push_back(ChatLine {
            role: Role::Assistant,
            text: answer,
        });
        while lines.len() > self.max_lines
            || lines.iter().map(|l| l.text.len()).sum::<usize>() > self.max_bytes
        {
            lines.pop_front();
            lines.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_stays_in_pairs_within_its_limits() {
        let limits = Limits {
            history_messages: 4,
            history_bytes: 100,
            ..Limits::default()
        };
        let history = History::new(&limits);
        for n in 0..4 {
            history.finished("c", format!("q{n}"), format!("a{n}"));
        }
        let lines = history.with_next("c", "next");
        let words: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(words, ["q2", "a2", "q3", "a3", "next"]);
        assert_eq!(lines[0].role, Role::User);
        assert_eq!(history.with_next("other", "x").len(), 1);
        history.finished("c", "x".repeat(200), "y".into());
        assert_eq!(history.with_next("c", "n").len(), 1, "over the bytes");
    }
}
