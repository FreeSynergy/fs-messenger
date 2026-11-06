//! The Anthropic Messages API, streaming: `POST /v1/messages`.
//!
//! Pattern: Strategy (one [`Dialect`]) + State: a reader walks the documented event flow and refuses a
//! stream that repeats or leaves it. Unknown event and delta types are ignored, as the versioning policy
//! asks. The format comes from documentation read on 2026-11-06, see `DESIGN.md`. Not checked against a
//! real server.

use std::collections::{HashMap, HashSet};

use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{Value, json};

use super::dialect::{
    ChatLine, Dialect, Ending, ModelEvent, StreamReader, TurnUsage, bearer, short_name,
};
use super::sse::SseEvent;
use crate::config::ModelConfig;
use crate::error::AiError;
use crate::secret::ApiKey;
use crate::turn::TurnFailure;

const API_VERSION: &str = "2026-10-21";

/// The dialect.
pub(crate) struct Anthropic;

impl Dialect for Anthropic {
    fn path(&self) -> &'static str {
        "/v1/messages"
    }

    fn headers(&self, key: Option<&ApiKey>) -> Result<Vec<(HeaderName, HeaderValue)>, AiError> {
        let key = key.ok_or_else(|| AiError::Config("this API needs a key".into()))?;
        Ok(vec![
            bearer(key)?,
            (
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static(API_VERSION),
            ),
        ])
    }

    fn body(&self, config: &ModelConfig, lines: &[ChatLine]) -> Value {
        let messages: Vec<Value> = lines
            .iter()
            .map(|l| json!({"role": l.role.as_str(), "content": l.text}))
            .collect();
        let mut body = json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "stream": true,
            "messages": messages,
        });
        if let Some(system) = &config.system {
            body["system"] = json!(system);
        }
        if config.ask_reasoning {
            body["thinking"] = json!({"type": "adaptive", "display": "summarized"});
        }
        if !config.tools.is_empty() {
            let tools: Vec<Value> = config
                .tools
                .iter()
                .map(|t| {
                    json!({"name": t.name, "description": t.description,
                        "input_schema": t.input_schema})
                })
                .collect();
            body["tools"] = json!(tools);
        }
        body
    }

    fn reader(&self) -> Box<dyn StreamReader> {
        Box::<Reader>::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Thinking,
    Tool,
    /// A block type this version does not read.
    Other,
}

#[derive(Default)]
struct Reader {
    started: bool,
    open: HashMap<u64, Block>,
    seen: HashSet<u64>,
    stop_reason: Option<String>,
    usage: TurnUsage,
}

fn malformed(what: &str) -> TurnFailure {
    TurnFailure::Malformed(what.to_string())
}

fn number(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn text_of<'a>(v: &'a Value, key: &str) -> Result<&'a str, TurnFailure> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("a delta without its text"))
}

impl Reader {
    fn start(&mut self, data: &Value) -> Result<Vec<ModelEvent>, TurnFailure> {
        if std::mem::replace(&mut self.started, true) {
            return Err(TurnFailure::Repeated("a second message_start".into()));
        }
        let usage = data.pointer("/message/usage").unwrap_or(&Value::Null);
        // The documented total: all three counts together.
        self.usage.input_tokens = number(usage, "input_tokens")
            .saturating_add(number(usage, "cache_creation_input_tokens"))
            .saturating_add(number(usage, "cache_read_input_tokens"));
        self.usage.output_tokens = number(usage, "output_tokens");
        Ok(vec![])
    }

    fn block_start(&mut self, data: &Value) -> Result<Vec<ModelEvent>, TurnFailure> {
        let index = self.index(data)?;
        if !self.seen.insert(index) {
            return Err(TurnFailure::Repeated("a content block twice".into()));
        }
        let block = data.get("content_block").unwrap_or(&Value::Null);
        let word = |key| block.get(key).and_then(Value::as_str).unwrap_or_default();
        let (kind, events) = match word("type") {
            "text" => {
                let events = match word("text") {
                    "" => vec![],
                    start => vec![ModelEvent::Text(start.to_string())],
                };
                (Block::Text, events)
            }
            "thinking" => (Block::Thinking, vec![]),
            "tool_use" => {
                let event = ModelEvent::ToolStart {
                    index,
                    agent_ref: word("id").to_string(),
                    name: word("name").to_string(),
                };
                (Block::Tool, vec![event])
            }
            _ => (Block::Other, vec![]),
        };
        self.open.insert(index, kind);
        Ok(events)
    }

    fn index(&self, data: &Value) -> Result<u64, TurnFailure> {
        if !self.started {
            return Err(malformed("content before message_start"));
        }
        data.get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| malformed("a block without an index"))
    }

    fn delta(&mut self, data: &Value) -> Result<Vec<ModelEvent>, TurnFailure> {
        let index = self.index(data)?;
        let block = *self
            .open
            .get(&index)
            .ok_or_else(|| malformed("a delta for a block that is not open"))?;
        let delta = data.get("delta").unwrap_or(&Value::Null);
        let kind = delta.get("type").and_then(Value::as_str).unwrap_or("");
        let event = match (kind, block) {
            ("text_delta", Block::Text) => ModelEvent::Text(text_of(delta, "text")?.to_string()),
            ("thinking_delta", Block::Thinking) => {
                ModelEvent::Reasoning(text_of(delta, "thinking")?.to_string())
            }
            ("input_json_delta", Block::Tool) => ModelEvent::ToolArguments {
                index,
                part: text_of(delta, "partial_json")?.to_string(),
            },
            ("text_delta" | "thinking_delta" | "input_json_delta", _) => {
                return Err(malformed("a delta of the wrong type for its block"));
            }
            // `signature_delta`, or one that a later API version added.
            _ => return Ok(vec![]),
        };
        Ok(vec![event])
    }

    fn block_stop(&mut self, data: &Value) -> Result<Vec<ModelEvent>, TurnFailure> {
        let index = self.index(data)?;
        match self.open.remove(&index) {
            Some(Block::Tool) => Ok(vec![ModelEvent::ToolEnd { index }]),
            Some(_) => Ok(vec![]),
            None => Err(malformed("a stop for a block that is not open")),
        }
    }

    fn message_delta(&mut self, data: &Value) -> Result<Vec<ModelEvent>, TurnFailure> {
        if !self.started {
            return Err(malformed("content before message_start"));
        }
        if let Some(reason) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
            self.stop_reason = Some(short_name(Some(reason)));
        }
        let mut events = vec![];
        if let Some(usage) = data.get("usage") {
            // Cumulative, as documented: this replaces what was said before.
            if usage.get("output_tokens").is_some() {
                self.usage.output_tokens = number(usage, "output_tokens");
            }
            self.usage.context_used = self.usage.input_tokens + self.usage.output_tokens;
            events.push(ModelEvent::Usage(self.usage));
        }
        Ok(events)
    }

    fn stop(&mut self) -> Result<Vec<ModelEvent>, TurnFailure> {
        if !self.started {
            return Err(malformed("content before message_start"));
        }
        let ending = match self.stop_reason.as_deref() {
            Some("end_turn" | "stop_sequence" | "tool_use") => Ending::Complete,
            Some(reason) => Ending::Cut(reason.to_string()),
            None => Ending::Cut("no stop reason".to_string()),
        };
        Ok(vec![ModelEvent::End(ending)])
    }
}

impl StreamReader for Reader {
    fn event(&mut self, event: &SseEvent) -> Result<Vec<ModelEvent>, TurnFailure> {
        let data: Value =
            serde_json::from_str(&event.data).map_err(|_| malformed("an event is not JSON"))?;
        let kind = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("an event without a type"))?;
        match kind {
            "message_start" => self.start(&data),
            "content_block_start" => self.block_start(&data),
            "content_block_delta" => self.delta(&data),
            "content_block_stop" => self.block_stop(&data),
            "message_delta" => self.message_delta(&data),
            "message_stop" => self.stop(),
            "error" => {
                let word = data.pointer("/error/type").and_then(Value::as_str);
                Err(TurnFailure::ServerError(short_name(word)))
            }
            // `ping`, or an event type a later API version added.
            _ => Ok(vec![]),
        }
    }

    fn end_of_body(&mut self) -> Result<Vec<ModelEvent>, TurnFailure> {
        Err(TurnFailure::BrokeOff)
    }
}
