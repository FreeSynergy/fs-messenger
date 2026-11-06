//! Tool calls as a stream writes them. Shown, never run.
//!
//! Pattern: Builder. What comes out takes the engine's fields from nowhere: the core sets id, revision,
//! risk, detail and options when it is recorded. The model's name and arguments are agent text.

use std::collections::HashMap;

use fs_messenger_core::{ToolCall, ToolCallOutcome, ToolCallState};

use crate::limits::Limits;
use crate::redact::Redactor;

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
