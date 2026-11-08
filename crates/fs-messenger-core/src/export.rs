//! Take everything with you (accepted idea 10, ADR 0046): export a conversation in an open format.
//!
//! Pattern: Strategy-free pure function (a Specification of the file format). **Format chosen:** JSON Lines,
//! one `ActivityStreams` 2.0 object per line (W3C Recommendation, `application/activity+json`): our envelope
//! already is AS2 (ADR 0027), so nothing is invented, any reader of AS2 or plain JSON can use it, and a
//! line-per-message file can be cut, appended and streamed. For mail the original is also kept byte for
//! byte in `original` (ADR 0030); mbox (RFC 4155) is the format to ask a mail provider for and is not
//! rebuilt here.

use serde_json::{Value, json};

use crate::model::Message;

/// The AS2 context.
pub const AS2_CONTEXT: &str = "https://www.w3.org/ns/activitystreams";

/// One message as an AS2 object: the envelope, the text, the media type, the names of the files.
///
/// # Errors
/// If the envelope cannot be written as JSON (it always can; the type says so for the caller).
pub fn message_object(message: &Message) -> Result<Value, serde_json::Error> {
    let mut object = serde_json::to_value(&message.summary.envelope)?;
    if let Some(map) = object.as_object_mut() {
        map.insert("@context".into(), json!(AS2_CONTEXT));
        map.insert("type".into(), json!("Note"));
        map.insert("content".into(), json!(message.body_text));
        let media = if message.body_media_type.is_empty() {
            "text/plain"
        } else {
            message.body_media_type.as_str()
        };
        map.insert("mediaType".into(), json!(media));
        map.insert(
            "fsEncryption".into(),
            serde_json::to_value(message.summary.encryption)?,
        );
        map.insert("fsOutgoing".into(), json!(message.outgoing));
        // The names of the files, not their bytes: a conversation file stays readable and small.
        if !message.attachments.is_empty() {
            let files: Vec<Value> = message
                .attachments
                .iter()
                .map(|a| {
                    json!({"type": "Document", "name": a.name, "mediaType": a.media_type, "size": a.size})
                })
                .collect();
            map.insert("attachment".into(), Value::Array(files));
        }
    }
    Ok(object)
}

/// A conversation as JSON Lines, oldest first as given: one object and a newline per message.
///
/// # Errors
/// As [`message_object`].
pub fn conversation_jsonl(messages: &[Message]) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for message in messages {
        out.push_str(&serde_json::to_string(&message_object(message)?)?);
        out.push('\n');
    }
    Ok(out)
}
