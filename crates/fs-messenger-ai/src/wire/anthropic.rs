//! The Anthropic Messages API, streaming: `POST /v1/messages`.
//!
//! Pattern: Strategy (one [`Dialect`]). The format comes from documentation read on 2026-11-06, see
//! `DESIGN.md`. Not checked against a real server. The stream's reader: `anthropic_stream.rs`.

use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{Value, json};

use super::anthropic_stream::Reader;
use super::dialect::{ChatLine, Dialect, StreamReader, bearer};
use crate::config::ModelConfig;
use crate::error::AiError;
use crate::secret::ApiKey;

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
