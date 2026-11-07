//! Sixth review, findings 2 and 3: a room the account has left is read down to the leave and then no
//! more, not from its start at every retry; and the messages after a limited `leave` timeline come.
//! A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fs_messenger_core::{FetchBatch, Provider};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{
    account, answer, error, is_room, login, ordinary, own_membership, page, plain, room_id,
    support, timeline,
};
use serde_json::json;

async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(account(server), &Password::new("pw"), store)
        .await
        .unwrap()
}

fn ids(batch: &FetchBatch) -> Vec<String> {
    batch.messages.iter().map(|m| m.aliases[0].id.clone()).collect()
}

/// A joins at s1, is left at s2 (the leave timeline is limited, `pl`), B joins at s3.
fn leave_then_b(broken: Arc<AtomicBool>) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s3") => answer("s4", &json!({})),
        p if p.contains("since=s2") => {
            let t = timeline(&[plain("$b1", "hi")], false, Some("pb"));
            answer("s3", &json!({"join": {room_id("b"): t}}))
        }
        p if p.contains("since=s1") => {
            let t = timeline(&[own_membership("$leave", "leave")], true, Some("pl"));
            answer("s2", &json!({"leave": {room_id("a"): t}}))
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$a1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("a"): t}}))
        }
        p if p.contains("/messages") && is_room(p, "a") && p.contains("from=pl") => {
            page(&[plain("$a2", "before the leave")])
        }
        p if p.contains("/messages") && is_room(p, "a") => page(&[plain("$old", "start")]),
        p if p.contains("/messages") && broken.load(Ordering::SeqCst) => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => page(&[plain("$b1", "hi")]),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn a_left_room_is_not_read_from_its_start_after_a_failed_fetch() {
    let broken = Arc::new(AtomicBool::new(true));
    let server = Server::start(leave_then_b(broken.clone())).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    assert!(provider.fetch(two.next.as_ref()).await.is_err(), "fetch 3 fails");
    broken.store(false, Ordering::SeqCst);
    let heard_before = server.heard().len();

    let mut cursor = two.next;
    for _ in 0..3 {
        cursor = provider.fetch(cursor.as_ref()).await.unwrap().next;
    }
    let from_start: Vec<_> = server.heard()[heard_before..]
        .iter()
        .filter(|(p, _)| p.contains("/messages") && is_room(p, "a") && !p.contains("to="))
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(from_start, Vec::<String>::new(), "a left room read to its start");
}

#[tokio::test]
async fn the_messages_before_a_limited_leave_are_read_to_the_old_position() {
    let server = Server::start(leave_then_b(Arc::new(AtomicBool::new(false)))).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();

    assert!(ids(&two).iter().any(|i| i.contains("$a2")), "lost: {:?}", ids(&two));
    let gap_read = server.heard().into_iter().find(|(p, _)| p.contains("from=pl"));
    let path = gap_read.expect("the gap was not read").0;
    assert!(path.contains("to=s1"), "not bounded by the old position: {path}");
}
