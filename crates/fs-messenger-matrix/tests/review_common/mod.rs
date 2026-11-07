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
pub async fn core(provider: MatrixProvider) -> (MessengerRepo, fs_store::ObjectId) {
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
