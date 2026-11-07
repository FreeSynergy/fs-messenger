//! Fifth review, finding 4: a gap in a room the SDK does not know is not removed silently. It is
//! given up by name (room and tokens, no content) in the cursor. A raw server, not Matrix.

mod review_common;

use fs_messenger_core::{Provider, ProviderCursor};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{account, login, ordinary, support};
use serde_json::{Value, json};

#[tokio::test]
async fn a_gap_in_an_unknown_room_is_named_when_it_is_given_up() {
    let server = Server::start(|path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => Reply::json(200, &json!({"next_batch": "s2"})),
        p => ordinary(p),
    })
    .await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let before = json!({"since": "s1", "gaps": [
        {"room": "!gone:x.test", "from": "t9", "to": "t1"}]});
    let batch = provider
        .fetch(Some(&ProviderCursor(before.to_string())))
        .await
        .unwrap();

    let after: Value = serde_json::from_str(&batch.next.unwrap().0).unwrap();
    assert_eq!(after.get("gaps"), None, "nothing is left to read");
    assert_eq!(
        after["given_up"],
        json!([{"room": "!gone:x.test", "from": "t9", "to": "t1"}]),
        "{after}"
    );
}
