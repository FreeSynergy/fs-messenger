//! What the scripted server plays: events written from documentation, not a real server's bytes.
#![allow(dead_code)]

/// Events in the form of the Anthropic documentation (streaming page, 2026-11-06).
pub mod anthropic {
    use serde_json::{Value, json};

    pub fn event(data: &Value) -> String {
        format!(
            "event: {}\ndata: {data}\n\n",
            data["type"].as_str().unwrap()
        )
    }
    pub fn start() -> String {
        event(
            &json!({"type": "message_start", "message": {"id": "msg_1", "type": "message",
            "role": "assistant", "content": [], "model": "scripted", "stop_reason": null,
            "stop_sequence": null, "usage": {"input_tokens": 25, "output_tokens": 1}}}),
        )
    }
    pub fn block(index: u64, kind: &str) -> String {
        event(&json!({"type": "content_block_start", "index": index,
            "content_block": {"type": kind, kind: ""}}))
    }
    pub fn tool(index: u64, id: &str, name: &str) -> String {
        event(&json!({"type": "content_block_start", "index": index,
            "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}}))
    }
    pub fn text(index: u64, text: &str) -> String {
        event(&json!({"type": "content_block_delta", "index": index,
            "delta": {"type": "text_delta", "text": text}}))
    }
    pub fn thinking(index: u64, text: &str) -> String {
        event(&json!({"type": "content_block_delta", "index": index,
            "delta": {"type": "thinking_delta", "thinking": text}}))
    }
    pub fn arguments(index: u64, part: &str) -> String {
        event(&json!({"type": "content_block_delta", "index": index,
            "delta": {"type": "input_json_delta", "partial_json": part}}))
    }
    pub fn stop_block(index: u64) -> String {
        event(&json!({"type": "content_block_stop", "index": index}))
    }
    pub fn end(reason: &str, output: u64) -> String {
        let delta = event(&json!({"type": "message_delta",
            "delta": {"stop_reason": reason, "stop_sequence": null},
            "usage": {"output_tokens": output}}));
        delta + &event(&json!({"type": "message_stop"}))
    }
    /// One text block, whole, ending well.
    pub fn answer(pieces: &[&str]) -> String {
        let mut all = start() + &block(0, "text");
        for piece in pieces {
            all += &text(0, piece);
        }
        all + &stop_block(0) + &end("end_turn", 15)
    }
}

/// Events in the form of llama.cpp's server: its README names the fields, `server-task.cpp` and
/// `tests/utils.py` at d02b4c0 show the chunks. Not a real server's bytes.
pub mod llama {
    use serde_json::{Value, json};

    pub fn chunk(delta: &Value, finish: Option<&str>) -> String {
        let data = json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1,
            "model": "scripted",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
        format!("data: {data}\n\n")
    }
    pub fn text(text: &str) -> String {
        chunk(&json!({"content": text}), None)
    }
    pub fn reasoning(text: &str) -> String {
        chunk(&json!({"reasoning_content": text}), None)
    }
    pub fn usage() -> String {
        let data = json!({"id": "chatcmpl-1", "object": "chat.completion.chunk", "choices": [],
            "usage": {"completion_tokens": 48, "prompt_tokens": 44, "total_tokens": 92},
            "timings": {"cache_n": 236, "prompt_n": 1, "predicted_n": 35}});
        format!("data: {data}\n\n")
    }
    pub fn end(finish: &str) -> String {
        chunk(&json!({}), Some(finish)) + &usage() + "data: [DONE]\n\n"
    }
    /// One answer, whole, ending well.
    pub fn answer(pieces: &[&str]) -> String {
        let mut all = chunk(&json!({"role": "assistant", "content": null}), None);
        for piece in pieces {
            all += &text(piece);
        }
        all + &end("stop")
    }
}
