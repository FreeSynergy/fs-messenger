//! llama.cpp's server, streaming: `POST /v1/chat/completions`.
//!
//! Pattern: Strategy (one [`Dialect`]) + State. Chosen over the native `/completion`, because there the
//! client would have to build the prompt from roles itself, which is how a message body becomes an
//! instruction; here the server's chat template keeps them apart. The README promises neither form as
//! stable. What it leaves to the documentation of `OpenAI` was read in its source, see `DESIGN.md`. Not checked
//! against a real server.

use std::collections::HashSet;

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

/// The dialect.
pub(crate) struct LlamaCpp;

impl Dialect for LlamaCpp {
    fn path(&self) -> &'static str {
        "/v1/chat/completions"
    }

    fn headers(&self, key: Option<&ApiKey>) -> Result<Vec<(HeaderName, HeaderValue)>, AiError> {
        key.map(bearer).into_iter().collect()
    }

    fn body(&self, config: &ModelConfig, lines: &[ChatLine]) -> Value {
        let mut messages: Vec<Value> = vec![];
        if let Some(system) = &config.system {
            messages.push(json!({"role": "system", "content": system}));
        }
        messages.extend(
            lines
                .iter()
                .map(|l| json!({"role": l.role.as_str(), "content": l.text})),
        );
        let mut body = json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
            "messages": messages,
        });
        if !config.tools.is_empty() {
            let tools: Vec<Value> = config
                .tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {"name": t.name,
                        "description": t.description, "parameters": t.input_schema}})
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

#[derive(Default)]
struct Reader {
    id: Option<String>,
    finish: Option<String>,
    tools: HashSet<u64>,
}

fn number(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

impl Reader {
    fn ending(&mut self) -> Result<Vec<ModelEvent>, TurnFailure> {
        let Some(finish) = self.finish.take() else {
            return Err(TurnFailure::BrokeOff);
        };
        let ending = match finish.as_str() {
            "stop" | "tool_calls" => Ending::Complete,
            other => Ending::Cut(other.to_string()),
        };
        let mut events: Vec<ModelEvent> = self
            .tools
            .drain()
            .map(|index| ModelEvent::ToolEnd { index })
            .collect();
        events.push(ModelEvent::End(ending));
        Ok(events)
    }

    fn tool(&mut self, call: &Value, events: &mut Vec<ModelEvent>) -> Result<(), TurnFailure> {
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| TurnFailure::Malformed("a tool call without an index".into()))?;
        let function = call.get("function").unwrap_or(&Value::Null);
        let word = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        if self.tools.insert(index) {
            events.push(ModelEvent::ToolStart {
                index,
                agent_ref: word(call, "id").unwrap_or_default(),
                name: word(function, "name").unwrap_or_default(),
            });
        }
        if let Some(part) = word(function, "arguments").filter(|p| !p.is_empty()) {
            events.push(ModelEvent::ToolArguments { index, part });
        }
        Ok(())
    }

    fn choice(&mut self, choice: &Value, events: &mut Vec<ModelEvent>) -> Result<(), TurnFailure> {
        if self.finish.is_some() {
            return Err(TurnFailure::Repeated("text after the finish".into()));
        }
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
            events.push(ModelEvent::Reasoning(text.to_string()));
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            events.push(ModelEvent::Text(text.to_string()));
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                self.tool(call, events)?;
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = Some(short_name(Some(reason)));
        }
        Ok(())
    }

    fn usage(data: &Value, usage: &Value) -> TurnUsage {
        // The README: "The total number of tokens in context is equal to prompt_n + cache_n +
        // predicted_n". Zero where a server does not say.
        let context_used = data.get("timings").map_or(0, |t| {
            number(t, "prompt_n")
                .saturating_add(number(t, "cache_n"))
                .saturating_add(number(t, "predicted_n"))
        });
        TurnUsage {
            input_tokens: number(usage, "prompt_tokens"),
            output_tokens: number(usage, "completion_tokens"),
            context_used,
        }
    }
}

impl StreamReader for Reader {
    fn event(&mut self, event: &SseEvent) -> Result<Vec<ModelEvent>, TurnFailure> {
        if event.data == "[DONE]" {
            return self.ending();
        }
        let data: Value = serde_json::from_str(&event.data)
            .map_err(|_| TurnFailure::Malformed("an event is not JSON".into()))?;
        if let Some(error) = data.get("error") {
            let word = error.get("type").and_then(Value::as_str);
            return Err(TurnFailure::ServerError(short_name(word)));
        }
        if !data.is_object() {
            return Err(TurnFailure::Malformed("an event is not an object".into()));
        }
        if let Some(id) = data.get("id").and_then(Value::as_str) {
            match &self.id {
                Some(held) if held != id => {
                    return Err(TurnFailure::Repeated("chunks of another completion".into()));
                }
                Some(_) => {}
                None => self.id = Some(id.to_string()),
            }
        }
        let mut events = vec![];
        if let Some(choices) = data.get("choices").and_then(Value::as_array) {
            // One answer was asked for. Others are not this turn's.
            let first = choices
                .iter()
                .find(|c| c.get("index").and_then(Value::as_u64).unwrap_or(0) == 0);
            if let Some(choice) = first {
                self.choice(choice, &mut events)?;
            }
        }
        if let Some(usage) = data.get("usage").filter(|u| u.is_object()) {
            events.push(ModelEvent::Usage(Self::usage(&data, usage)));
        }
        Ok(events)
    }

    /// Without `[DONE]`: the model's finish still counts, since it said so itself.
    fn end_of_body(&mut self) -> Result<Vec<ModelEvent>, TurnFailure> {
        self.ending()
    }
}
