//! Plain HTTP to the homeserver, past the SDK: registering users, and looking at what the server holds.

use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A setting: from the environment, else from `scripts/test-servers/env.sh`.
pub fn setting(name: &str) -> String {
    if let Ok(value) = std::env::var(name) {
        return value;
    }
    let env_sh = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/test-servers/env.sh"
    );
    let output = Command::new("bash")
        .args(["-c", &format!(". {env_sh}; printf %s \"${name}\"")])
        .stderr(Stdio::inherit())
        .output()
        .expect("read env.sh");
    String::from_utf8(output.stdout).unwrap()
}

/// The server's URL.
pub fn base() -> String {
    setting("FS_MSGTEST_MATRIX_URL")
}

/// One way to talk to the server: HTTP on loopback, with a TLS backend because the client needs one.
pub fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().build().unwrap()
}

async fn call(
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, String) {
    let url = format!("{}/_matrix/client/v3{path}", base());
    let mut request = client().request(method, url);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("server reachable");
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

/// `GET`: the status and the body, exactly as they came.
pub async fn get(path: &str, token: &str) -> (u16, String) {
    call(reqwest::Method::GET, path, Some(token), None).await
}

/// `POST`, answer as JSON.
pub async fn post(path: &str, token: Option<&str>, body: Value) -> Value {
    let (_, text) = call(reqwest::Method::POST, path, token, Some(body)).await;
    serde_json::from_str(&text).unwrap()
}

/// `PUT`, answer as JSON.
pub async fn put(path: &str, token: &str, body: Value) -> Value {
    let (_, text) = call(reqwest::Method::PUT, path, Some(token), Some(body)).await;
    serde_json::from_str(&text).unwrap()
}

/// Make a user with the registration flow of the test server.
pub async fn register(user: &str, password: &str) {
    let request = json!({"username": user, "password": password, "inhibit_login": true});
    let first = post("/register", None, request.clone()).await;
    let session = first["session"]
        .as_str()
        .unwrap_or_else(|| panic!("no flow: {first}"));
    let mut second = request;
    second["auth"] = json!({
        "type": "m.login.registration_token",
        "token": setting("FS_MSGTEST_MATRIX_REGISTRATION_TOKEN"),
        "session": session,
    });
    let answer = post("/register", None, second).await;
    assert!(
        answer["user_id"].is_string(),
        "registering {user}: {answer}"
    );
}

/// A login of its own: a device that never uploads keys, so nobody encrypts to it. It sees what the
/// server keeps, which is the point. Answers the access token.
pub async fn plain_login(user: &str, password: &str) -> String {
    let body = json!({
        "type": "m.login.password",
        "identifier": {"type": "m.id.user", "user": user},
        "password": password,
    });
    let answer = post("/login", None, body).await;
    answer["access_token"].as_str().unwrap().to_owned()
}

pub fn encode(id: &str) -> String {
    id.replace('!', "%21")
        .replace('$', "%24")
        .replace(':', "%3A")
}

/// An event as the server holds it, its bytes untouched.
pub async fn raw_event(token: &str, room: &str, event: &str) -> String {
    let path = format!("/rooms/{}/event/{}", encode(room), encode(event));
    let (status, body) = get(&path, token).await;
    assert_eq!(status, 200, "{body}");
    body
}

/// The newest events of a room, as the server holds them.
pub async fn room_events(token: &str, room: &str) -> Vec<Value> {
    let path = format!("/rooms/{}/messages?dir=b&limit=100", encode(room));
    let (status, body) = get(&path, token).await;
    assert_eq!(status, 200, "{body}");
    let parsed: Value = serde_json::from_str(&body).unwrap();
    parsed["chunk"].as_array().unwrap().clone()
}

/// Make a plain room: no encryption event at all. Answers its id.
pub async fn plain_room(token: &str, invite: &[&str]) -> String {
    let body = json!({"preset": "private_chat", "invite": invite});
    let answer = post("/createRoom", Some(token), body).await;
    answer["room_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
        .to_owned()
}

/// What another client does: a plain event straight into a room. Answers the event id.
pub async fn plain_message(token: &str, room: &str, text: &str) -> String {
    let nonce = fs_objects::ObjectId::new();
    let path = format!("/rooms/{}/send/m.room.message/{nonce}", encode(room));
    let answer = put(&path, token, json!({"msgtype": "m.text", "body": text})).await;
    answer["event_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
        .to_owned()
}

/// The devices of the user behind `token`.
pub async fn devices(token: &str) -> Vec<String> {
    let (status, body) = get("/devices", token).await;
    assert_eq!(status, 200, "{body}");
    let parsed: Value = serde_json::from_str(&body).unwrap();
    let list = parsed["devices"].as_array().unwrap();
    list.iter()
        .map(|d| d["device_id"].as_str().unwrap().to_owned())
        .collect()
}

/// A room-level action by a plain HTTP client (`leave`, `forget`, `invite`, `join` ...), body as given.
pub async fn room_action(token: &str, room: &str, action: &str, body: Value) -> Value {
    let path = format!("/rooms/{}/{action}", encode(room));
    post(&path, Some(token), body).await
}
