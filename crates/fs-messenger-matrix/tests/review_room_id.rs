//! Fifth review, finding 2: a room id with `|` or a control character is legal in Matrix. The
//! messages of such a room must reach the core, which refuses those characters in an id. A raw server.

mod review_common;

use fs_messenger_core::{Ingest, Messenger};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{account, core, login, ordinary, plain, support};
use serde_json::json;

async fn arrives_from(room: &'static str) {
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => {
            let timeline = json!({"events":[plain("$e1","hello")],"limited":false});
            let rooms = json!({"join": {room: {"timeline": timeline}}});
            Reply::json(200, &json!({"next_batch": "s1", "rooms": rooms}))
        }
        p => ordinary(p),
    })
    .await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let (repo, id) = core(provider).await;

    let report = repo.sync(id).await.unwrap();
    assert_eq!((report.stored, report.rejected), (1, 0), "{report:?}");
    let conversations = repo.conversations(None, false, None).await.unwrap().items;
    assert_eq!(conversations.len(), 1);
    let page = repo.messages(conversations[0].id, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn a_room_id_with_a_bar_arrives() {
    arrives_from("!a|b:x.test").await;
}

#[tokio::test]
async fn a_room_id_with_control_characters_arrives() {
    arrives_from("!a\nb\u{1}c:x.test").await;
}

#[tokio::test]
async fn a_room_id_with_a_slash_and_a_percent_arrives() {
    arrives_from("!a/b%7C:x.test").await;
}
