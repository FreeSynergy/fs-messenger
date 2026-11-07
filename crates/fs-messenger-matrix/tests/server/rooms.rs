//! Plain HTTP for the membership probes: a `/sync` by hand, the check's filtered `/messages`, rooms
//! with chosen state, and a knock. Past the provider and the SDK, like `http.rs`.

use serde_json::{Value, json};

use crate::http::{encode, get, post};

/// `GET /sync` by plain HTTP, no wait: status and parsed body. `since` is a token of an earlier answer.
pub async fn sync_raw(token: &str, since: Option<&str>) -> (u16, Value) {
    let mut path = String::from("/sync?timeout=0&set_presence=offline");
    if let Some(since) = since {
        path.push_str("&since=");
        path.push_str(since);
    }
    let (status, text) = get(&path, token).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// `GET /rooms/{room}/messages`, newest first, filtered to the member events of `sender`: status and body.
pub async fn own_member_events(token: &str, room: &str, sender: &str) -> (u16, Value) {
    let filter = json!({"types": ["m.room.member"], "senders": [sender]}).to_string();
    let filter = filter
        .replace('{', "%7B")
        .replace('}', "%7D")
        .replace('"', "%22");
    let filter = filter
        .replace(':', "%3A")
        .replace(',', "%2C")
        .replace('[', "%5B");
    let filter = filter.replace(']', "%5D").replace('@', "%40");
    let path = format!(
        "/rooms/{}/messages?dir=b&limit=50&filter={filter}",
        encode(room)
    );
    let (status, text) = get(&path, token).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A room with these `initial_state` events (`[(type, content)]`) and invitations. Answers its id.
pub async fn room_with(token: &str, invite: &[&str], state: &[(&str, Value)]) -> String {
    let initial: Vec<_> = state
        .iter()
        .map(|(kind, content)| json!({"type": kind, "state_key": "", "content": content}))
        .collect();
    let body = json!({"preset": "private_chat", "invite": invite, "initial_state": initial});
    let answer = post("/createRoom", Some(token), body).await;
    answer["room_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
        .to_owned()
}

/// `POST /knock/{room}` by plain HTTP.
pub async fn knock(token: &str, room: &str) -> Value {
    post(&format!("/knock/{}", encode(room)), Some(token), json!({})).await
}
