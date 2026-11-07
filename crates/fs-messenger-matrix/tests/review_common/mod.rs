//! Shared by the tests of the second review: a raw server that stands in for a homeserver, and a core
//! with the provider registered. Not Matrix: only the answers each test needs.
#![allow(dead_code)] // each test file uses a different subset

use std::sync::Arc;
use std::time::Duration;

use fs_messenger_core::{AccountSetup, Messenger, MessengerConfig, MessengerRepo};
use fs_messenger_matrix::{MatrixAccount, MatrixProvider};
use serde_json::{Value, json};

#[path = "../support/mod.rs"]
pub mod support;

use support::server::{Reply, Server};

pub const USER: &str = "@alice:x.test";
pub const TOKEN: &str = "token-needle-of-review";
pub const ROOM: &str = "!a:x.test";

pub fn login() -> Reply {
    let body = json!({"user_id": USER, "access_token": TOKEN, "device_id": "DEV"});
    Reply::json(200, &body)
}

pub fn ordinary(path: &str) -> Reply {
    if path.ends_with("/versions") {
        return Reply::json(200, &json!({"versions": ["v1.11"]}));
    }
    Reply::json(404, &json!({"errcode": "M_NOT_FOUND", "error": "no"}))
}

pub fn account(server: &Server) -> MatrixAccount {
    let mut account = MatrixAccount::new(server.url(), USER);
    account.request_timeout = Duration::from_secs(2);
    account
}

pub fn plain(id: &str, body: &str) -> Value {
    json!({"type":"m.room.message","event_id":id,"sender":"@bob:x.test","origin_server_ts":1000,
           "content":{"msgtype":"m.text","body":body}})
}

pub fn encrypted(id: &str) -> Value {
    json!({"type":"m.room.encrypted","event_id":id,"sender":"@bob:x.test","origin_server_ts":1000,
           "content":{"algorithm":"m.megolm.v1.aes-sha2","ciphertext":"AwgAEnAcz9V",
           "sender_key":"Gq0f0Qq0aQx0bQx0cQx0dQx0eQx0fQx0gQx0hQx0iQx","device_id":"BOBDEV",
           "session_id":"c2Vzc2lvbg"}})
}

/// A `/sync` answer with one joined room.
pub fn sync(next: &str, events: &[Value], limited: bool, prev: Option<&str>) -> Reply {
    let mut timeline = json!({"events": events, "limited": limited});
    if let Some(prev) = prev {
        timeline["prev_batch"] = json!(prev);
    }
    let rooms = json!({"join": {ROOM: {"timeline": timeline}}});
    Reply::json(200, &json!({"next_batch": next, "rooms": rooms}))
}

/// A core with this provider as its one account.
pub async fn core(provider: MatrixProvider) -> (MessengerRepo, fs_objects::ObjectId) {
    let messages = support::fresh_store().await;
    let config = MessengerConfig::new("https://local.invalid/u");
    let repo = MessengerRepo::open_turso(messages, config).await.unwrap();
    let setup = AccountSetup {
        address: USER.into(),
        kind: "matrix".into(),
        secret: String::new(),
    };
    let account = repo.add_account(setup).await.unwrap();
    repo.register_provider(account.id, Arc::new(provider));
    (repo, account.id)
}

/// A `/sync` answer with these rooms: `rooms` is `{"join": {...}, "leave": {...}}`.
pub fn answer(next: &str, rooms: &Value) -> Reply {
    Reply::json(200, &json!({"next_batch": next, "rooms": rooms}))
}

/// A timeline for `answer`.
pub fn timeline(events: &[Value], limited: bool, prev: Option<&str>) -> Value {
    let mut t = json!({"events": events, "limited": limited});
    if let Some(prev) = prev {
        t["prev_batch"] = json!(prev);
    }
    json!({"timeline": t})
}

/// The account's own membership event, as it comes in a left room's timeline.
pub fn own_membership(id: &str, membership: &str) -> Value {
    json!({"type":"m.room.member","event_id":id,"sender":"@carol:x.test","origin_server_ts":900,
           "state_key":USER,"content":{"membership":membership}})
}

/// The id of room `!<name>:x.test`.
pub fn room_id(name: &str) -> String {
    format!("!{name}:x.test")
}

/// Whether this request is for `/rooms/!<name>:x.test/...` (the id may be percent-encoded).
pub fn is_room(path: &str, name: &str) -> bool {
    path.contains(&format!("!{name}:x.test")) || path.contains(&format!("%21{name}%3Ax.test"))
}

/// A `/messages` answer with one page and no `end`.
pub fn page(events: &[Value]) -> Reply {
    Reply::json(200, &json!({"start":"x","chunk":events}))
}

pub fn error(status: u16, code: &str) -> Reply {
    Reply::json(status, &json!({"errcode": code, "error": "no"}))
}

/// The account's own membership event, sent by the account itself. `previous` is the membership it
/// replaced (`unsigned.prev_content`), as a homeserver adds it.
pub fn own_event(id: &str, membership: &str, previous: Option<&str>) -> Value {
    let mut event = json!({"type":"m.room.member","event_id":id,"sender":USER,"origin_server_ts":900,
           "state_key":USER,"content":{"membership":membership}});
    if let Some(previous) = previous {
        event["unsigned"] = json!({"prev_content": {"membership": previous}});
    }
    event
}

/// Whether this request is a check: `/messages` filtered to member events.
pub fn is_check(path: &str) -> bool {
    path.contains("/messages") && path.contains("m.room.member")
}

/// A provider logged in at this server, on a store of its own.
pub async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(
        account(server),
        &fs_messenger_matrix::Password::new("pw"),
        store,
    )
    .await
    .unwrap()
}

/// The event ids of a batch.
pub fn ids(batch: &fs_messenger_core::FetchBatch) -> Vec<String> {
    batch
        .messages
        .iter()
        .map(|m| m.aliases[0].id.clone())
        .collect()
}

/// The cursor of a batch as JSON.
pub fn cursor_json(batch: &fs_messenger_core::FetchBatch) -> Value {
    serde_json::from_str(&batch.next.as_ref().unwrap().0).unwrap()
}

/// The paths of the `/messages` requests the server heard for room `!<name>:x.test`, checks and reads.
pub fn messages_for(server: &Server, name: &str) -> Vec<String> {
    let heard = server.heard().into_iter().map(|(p, _)| p);
    heard
        .filter(|p| p.contains("/messages") && is_room(p, name))
        .collect()
}

/// Of these, the reads that are not checks and have no end (`to=`): from the start of the room.
pub fn from_the_start(paths: &[String]) -> Vec<&String> {
    let open = |p: &&String| !is_check(p) && !p.contains("to=");
    paths.iter().filter(open).collect()
}

/// The paths of `/sync` requests heard from the `n`-th request on.
pub fn syncs_after(server: &Server, skip: usize) -> Vec<String> {
    let heard = server.heard().into_iter().map(|(p, _)| p);
    heard.skip(skip).filter(|p| p.contains("/sync")).collect()
}

/// Whether this `/sync` is the SDK's own (it asks for `use_state_after`); ours does not.
pub fn is_sdk_sync(path: &str) -> bool {
    path.contains("use_state_after")
}
