//! What this provider reads out of an event's JSON.
//!
//! Pattern: Specification: pure rules over untrusted JSON. A field of the wrong shape counts as
//! absent, and the event as "not a message", so a stranger's odd event cannot fail a sync.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// An event as far as this provider cares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedEvent {
    pub(crate) event_type: String,
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) published: Option<DateTime<Utc>>,
    /// The text to show; `None` if this is no readable message, for example after a redaction.
    pub(crate) body: Option<String>,
    /// What it answers: `m.in_reply_to`, or the target of an edit.
    pub(crate) in_reply_to: Option<String>,
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut at = value;
    for step in path {
        at = at.get(step)?;
    }
    at.as_str()
}

fn body_of(content: &Value) -> Option<String> {
    // An edit's real text is in `m.new_content`; `body` is a fallback that starts with "* ".
    let replacing = text(content, &["m.relates_to", "rel_type"]) == Some("m.replace");
    let from = if replacing {
        content.get("m.new_content").unwrap_or(content)
    } else {
        content
    };
    let body = text(from, &["body"])?;
    match text(from, &["msgtype"])? {
        "m.text" | "m.notice" | "m.emote" => Some(body.to_owned()),
        // Files are not fetched (`DESIGN.md`): their name shows, and that it is one.
        other => Some(format!("[{other}] {body}")),
    }
}

fn parent_of(content: &Value) -> Option<String> {
    let reply = text(content, &["m.relates_to", "m.in_reply_to", "event_id"]);
    let edit = (text(content, &["m.relates_to", "rel_type"]) == Some("m.replace"))
        .then(|| text(content, &["m.relates_to", "event_id"]))
        .flatten();
    reply.or(edit).map(ToOwned::to_owned)
}

/// Read one event. `None` if it is not JSON or has no type, id and sender.
pub(crate) fn parse(json: &str) -> Option<ParsedEvent> {
    let value: Value = serde_json::from_str(json).ok()?;
    let content = value.get("content").cloned().unwrap_or(Value::Null);
    let published = value
        .get("origin_server_ts")
        .and_then(Value::as_i64)
        .and_then(DateTime::from_timestamp_millis);
    Some(ParsedEvent {
        event_type: text(&value, &["type"])?.to_owned(),
        event_id: text(&value, &["event_id"])?.to_owned(),
        sender: text(&value, &["sender"])?.to_owned(),
        published,
        body: body_of(&content),
        in_reply_to: parent_of(&content),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_names_its_parent() {
        let e = parse(
            r#"{"type":"m.room.message","event_id":"$b","sender":"@a:x","origin_server_ts":1000,
            "content":{"msgtype":"m.text","body":"hi","m.relates_to":{"m.in_reply_to":{"event_id":"$a"}}}}"#,
        )
        .unwrap();
        assert_eq!(e.body.as_deref(), Some("hi"));
        assert_eq!(e.in_reply_to.as_deref(), Some("$a"));
        assert_eq!(e.published.unwrap().timestamp_millis(), 1000);
    }

    #[test]
    fn an_edit_shows_its_new_text_as_a_child_of_its_target() {
        let e = parse(
            r#"{"type":"m.room.message","event_id":"$b","sender":"@a:x","content":{"msgtype":"m.text",
            "body":"* new","m.new_content":{"msgtype":"m.text","body":"new"},
            "m.relates_to":{"rel_type":"m.replace","event_id":"$a"}}}"#,
        )
        .unwrap();
        assert_eq!(e.body.as_deref(), Some("new"));
        assert_eq!(e.in_reply_to.as_deref(), Some("$a"));
    }

    #[test]
    fn odd_shapes_are_no_message_and_no_failure() {
        let redacted =
            parse(r#"{"type":"m.room.message","event_id":"$b","sender":"@a:x","content":{}}"#);
        assert_eq!(redacted.unwrap().body, None);
        let wrong = parse(
            r#"{"type":"m.room.message","event_id":"$b","sender":"@a:x","content":{"msgtype":1,"body":[]}}"#,
        );
        assert_eq!(wrong.unwrap().body, None);
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"type":"m.room.message"}"#).is_none());
    }

    #[test]
    fn a_file_says_it_is_one() {
        let e = parse(r#"{"type":"m.room.message","event_id":"$b","sender":"@a:x","content":{"msgtype":"m.image","body":"cat.png"}}"#).unwrap();
        assert_eq!(e.body.as_deref(), Some("[m.image] cat.png"));
    }
}
