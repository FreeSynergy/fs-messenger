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
        json!([{"room": "!gone:x.test", "from": "t9", "to": "t1", "why": "unknown_room"}]),
        "{after}"
    );
}

/// Every sync brings the same limited room (`prev_batch` set) so that a gap is planned again each
/// fetch; `/messages` of it is refused.
fn refusing(rooms: usize, again: bool) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    let syncs = std::sync::atomic::AtomicUsize::new(0);
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => {
            let n = syncs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // With `again` every answer brings the rooms (and so their gaps) anew; else only the first.
            let count = if again || n == 0 { rooms } else { 0 };
            let all: serde_json::Map<_, _> = (0..count)
                .map(|i| {
                    let t = review_common::timeline(&[], true, Some("pf"));
                    (review_common::room_id(&format!("f{i}")), t)
                })
                .collect();
            // The SDK drops an answer whose `next_batch` it has seen: every answer has its own.
            review_common::answer(&format!("s{n}"), &json!({"join": all}))
        }
        p if p.contains("/messages") => review_common::error(403, "M_FORBIDDEN"),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn forbidden_is_named_once_per_room() {
    let server = Server::start(refusing(1, true)).await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let mut cursor = None;
    let mut last = Value::Null;
    for _ in 0..5 {
        let batch = provider.fetch(cursor.as_ref()).await.unwrap();
        last = serde_json::from_str(&batch.next.as_ref().unwrap().0).unwrap();
        cursor = batch.next;
    }
    let named = last["given_up"].as_array().unwrap();
    assert_eq!(named.len(), 1, "{last}");
    assert_eq!(named[0]["why"], "forbidden");
    assert_eq!(named[0]["times"], 5, "{last}");
}

#[tokio::test]
async fn forty_forbidden_rooms_are_named_up_to_the_bound() {
    let server = Server::start(refusing(40, false)).await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let mut cursor = None;
    let mut last = Value::Null;
    for _ in 0..4 {
        let batch = provider.fetch(cursor.as_ref()).await.unwrap();
        last = serde_json::from_str(&batch.next.as_ref().unwrap().0).unwrap();
        cursor = batch.next;
    }
    let named = last["given_up"].as_array().unwrap();
    assert_eq!(named.len(), 32, "{} named", named.len());
    let rooms: std::collections::HashSet<_> = named.iter().map(|g| g["room"].to_string()).collect();
    assert_eq!(rooms.len(), 32, "one entry per room");
}
