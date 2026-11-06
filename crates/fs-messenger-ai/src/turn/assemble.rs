//! What a stream says, put together: text in pieces on whole code points, and tool calls.
//!
//! Pattern: Builder. Offsets are in UTF-8 bytes per part, as in the contract's `TextChunk`, and they count
//! what was really handed on: without the key, without NUL (the core would take that out too, and the
//! stored text must equal the pieces), and without what waits because it may be the start of the key.

use std::collections::HashMap;

use fs_messenger_core::{ToolCall, ToolCallOutcome, ToolCallState};

use crate::limits::Limits;
use crate::redact::Redactor;

/// One text being written: the answer, or the reasoning.
pub(crate) struct PartText {
    sent: u64,
    waiting: String,
    cap: usize,
    cut: bool,
}

impl PartText {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            sent: 0,
            waiting: String::new(),
            cap,
            cut: false,
        }
    }

    /// The piece to hand on now, with its offset. `None` if nothing is ready.
    pub(crate) fn push(&mut self, delta: &str, redactor: &Redactor) -> Option<(u64, String)> {
        if self.cut {
            return None;
        }
        self.waiting.push_str(&delta.replace('\0', ""));
        self.waiting = redactor.scrubbed(&self.waiting);
        let ready = self.waiting.len() - redactor.held_back(&self.waiting);
        let rest = self.waiting.split_off(ready);
        let piece = std::mem::replace(&mut self.waiting, rest);
        self.hand_on(piece)
    }

    /// What was still waiting: the stream has ended, so it cannot become the key any more.
    pub(crate) fn flush(&mut self) -> Option<(u64, String)> {
        let piece = std::mem::take(&mut self.waiting);
        self.hand_on(piece)
    }

    fn hand_on(&mut self, mut piece: String) -> Option<(u64, String)> {
        let room = self
            .cap
            .saturating_sub(usize::try_from(self.sent).unwrap_or(usize::MAX));
        if piece.len() > room {
            let at = (0..=room)
                .rev()
                .find(|&i| piece.is_char_boundary(i))
                .unwrap_or(0);
            piece.truncate(at);
            self.cut = true;
        }
        if piece.is_empty() {
            return None;
        }
        let offset = self.sent;
        self.sent += piece.len() as u64;
        Some((offset, piece))
    }

    /// Whether the cap took some of it.
    pub(crate) fn was_cut(&self) -> bool {
        self.cut
    }
}

struct Building {
    agent_ref: String,
    name: String,
    arguments: String,
    ended: bool,
}

/// Tool calls as a stream writes them. Shown, never run.
pub(crate) struct ToolCalls {
    by_index: HashMap<u64, usize>,
    calls: Vec<Building>,
    max_calls: usize,
    max_arguments: usize,
}

const NOT_RUN: &str = "Not run: this provider runs no tools.";
const MAX_NAME_CHARS: usize = 256;

fn clean(text: &str, redactor: &Redactor) -> String {
    redactor
        .scrubbed(text)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect()
}

impl ToolCalls {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            by_index: HashMap::new(),
            calls: vec![],
            max_calls: limits.tool_calls,
            max_arguments: limits.tool_argument_bytes,
        }
    }

    /// One more; past the limit it is not shown.
    pub(crate) fn start(&mut self, index: u64, agent_ref: &str, name: &str) {
        if self.calls.len() < self.max_calls {
            self.by_index.insert(index, self.calls.len());
            self.calls.push(Building {
                agent_ref: agent_ref.to_string(),
                name: name.to_string(),
                arguments: String::new(),
                ended: false,
            });
        }
    }

    pub(crate) fn arguments(&mut self, index: u64, part: &str) {
        let Some(&at) = self.by_index.get(&index) else {
            return;
        };
        let call = &mut self.calls[at];
        let room = self.max_arguments.saturating_sub(call.arguments.len());
        let fits = (0..=room.min(part.len()))
            .rev()
            .find(|&i| part.is_char_boundary(i))
            .unwrap_or(0);
        call.arguments.push_str(&part[..fits]);
    }

    pub(crate) fn end(&mut self, index: u64) {
        if let Some(&at) = self.by_index.get(&index) {
            self.calls[at].ended = true;
        }
    }

    /// What goes to `record_tool_call`, with its reference: the model's id, under our position, so a
    /// repeated id cannot overwrite another call. Everything else the engine decides. A call nothing runs
    /// is over at once: `refused`, or `failed` if the stream broke while it was being written.
    pub(crate) fn finished(self, redactor: &Redactor) -> Vec<(String, ToolCall)> {
        self.calls
            .into_iter()
            .enumerate()
            .map(|(at, call)| {
                let outcome = if call.ended {
                    ToolCallOutcome::Refused
                } else {
                    ToolCallOutcome::Failed
                };
                let reference = format!("{at}:{}", clean(&call.agent_ref, redactor));
                let shown = ToolCall {
                    name: clean(&call.name, redactor),
                    state: ToolCallState::Settled,
                    outcome,
                    summary: redactor.scrubbed(&call.arguments).replace('\0', ""),
                    result: NOT_RUN.to_string(),
                    ..ToolCall::default()
                };
                (reference, shown)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::ApiKey;

    #[test]
    fn offsets_count_bytes_and_a_key_over_two_pieces_never_shows() {
        let r = Redactor::new(Some(ApiKey::new("sk-key")));
        let mut part = PartText::new(usize::MAX);
        assert_eq!(part.push("gr\u{fc}", &r), Some((0, "gr\u{fc}".into())));
        assert_eq!(part.push("\u{df} s", &r), Some((4, "\u{df} ".into())));
        assert_eq!(part.push("k-", &r), None);
        assert_eq!(part.push("key!", &r), Some((7, "[redacted]!".into())));
        assert_eq!(part.push(" sk", &r), Some((18, " ".into())));
        assert_eq!(part.flush(), Some((19, "sk".into())));
    }

    #[test]
    fn the_cap_ends_on_a_whole_code_point_and_says_so() {
        let r = Redactor::default();
        let mut part = PartText::new(3);
        assert_eq!(part.push("a\u{1f600}", &r), Some((0, "a".into())));
        assert!(part.was_cut());
        assert_eq!(part.push("b", &r), None);
    }

    #[test]
    fn nul_never_counts() {
        let r = Redactor::default();
        let mut part = PartText::new(usize::MAX);
        assert_eq!(part.push("a\0b", &r), Some((0, "ab".into())));
    }
}
