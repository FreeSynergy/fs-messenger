//! Sixth review, finding 1: a room the account may not read any more (banned, forgotten) must never
//! block the account. A `M_FORBIDDEN` on `/messages` ends that room's gap by name; other errors stay
//! repeatable. A raw server, not Matrix; what Synapse really answers was not asked.
//!
//! Seventh review, finding 2: a ban reaches the provider under `leave` (sync v2 never says `Banned`;
//! the SDK marks every room of `leave` as left), so the first test below proves the 403 give-up for a
//! left room, not a `Banned` branch (there is none: the room-state table has no ban).

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
use serde_json::{Value, json};

async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(account(server), &Password::new("pw"), store)
        .await
        .unwrap()
}

fn ids(batch: &FetchBatch) -> Vec<String> {
    batch
        .messages
        .iter()
        .map(|m| m.aliases[0].id.clone())
        .collect()
}

/// Banned from `!a`, then B joins; B's first reading fails while `broken`.
fn ban_then_b(broken: Arc<AtomicBool>) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s3") => answer("s4", &json!({})),
        p if p.contains("since=s2") => {
            let t = timeline(&[plain("$b1", "hi")], false, Some("pb"));
            answer("s3", &json!({"join": {room_id("b"): t}}))
        }
        p if p.contains("since=s1") => {
            let t = timeline(&[own_membership("$ban", "ban")], true, Some("pa2"));
            answer("s2", &json!({"leave": {room_id("a"): t}}))
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$a1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("a"): t}}))
        }
        p if p.contains("/messages") && is_room(p, "a") => error(403, "M_FORBIDDEN"),
        p if p.contains("/messages") && broken.load(Ordering::SeqCst) => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => page(&[plain("$b1", "hi")]),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn a_ban_seen_as_a_leave_and_one_failed_fetch_do_not_block_the_account() {
    let broken = Arc::new(AtomicBool::new(true));
    let server = Server::start(ban_then_b(broken.clone())).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    assert!(
        provider.fetch(two.next.as_ref()).await.is_err(),
        "fetch 3 fails"
    );
    broken.store(false, Ordering::SeqCst);

    let mut cursor = two.next;
    let mut seen = Vec::new();
    for n in 0..3 {
        let batch = provider
            .fetch(cursor.as_ref())
            .await
            .unwrap_or_else(|e| panic!("fetch {} after the failure: {e}", n + 1));
        seen.extend(ids(&batch));
        cursor = batch.next;
    }
    assert!(seen.iter().any(|i| i.contains("$b1")), "lost: {seen:?}");
    // A limited ban timeline is read down to the old position (never from the start); the 403 gives
    // the room up by name, once, instead of blocking this and the fetches after the failure.
    let read_a = |p: &str| p.contains("/messages") && is_room(p, "a") && p.contains("from=pa2");
    let heard: Vec<String> = server
        .heard()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| read_a(p))
        .collect();
    assert!(!heard.is_empty(), "the limited timeline was to be read");
    assert!(heard.iter().all(|p| p.contains("to=s1")), "{heard:?}");
    let cursor: Value = serde_json::from_str(&cursor.unwrap().0).unwrap();
    assert!(
        cursor["given_up"].to_string().contains("!a:x.test"),
        "{cursor}"
    );
}

/// Two joined rooms with limited timelines; `!a` answers `a_status`, `!b` has a message.
fn two_rooms(a_status: u16, code: &'static str) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=") => answer("s2", &json!({})),
        p if p.contains("/sync") => {
            let (a, b) = (
                timeline(&[], true, Some("pa")),
                timeline(&[], true, Some("pb")),
            );
            answer("s1", &json!({"join": {room_id("a"): a, room_id("b"): b}}))
        }
        p if p.contains("/messages") && is_room(p, "a") => error(a_status, code),
        p if p.contains("/messages") => page(&[plain("$b1", "hi")]),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn a_forbidden_room_is_named_and_the_others_arrive() {
    let server = Server::start(two_rooms(403, "M_FORBIDDEN")).await;
    let provider = provider(&server).await;
    let batch = provider
        .fetch(None)
        .await
        .expect("forbidden must not fail the fetch");
    assert!(
        ids(&batch).iter().any(|i| i.contains("$b1")),
        "{:?}",
        ids(&batch)
    );

    let cursor: Value = serde_json::from_str(&batch.next.unwrap().0).unwrap();
    assert_eq!(cursor.get("gaps"), None, "{cursor}");
    let named = cursor["given_up"].to_string();
    assert!(
        named.contains("!a:x.test") && !named.contains("!b:x.test"),
        "{named}"
    );
}

#[tokio::test]
async fn an_error_that_may_pass_stays_repeatable() {
    // A 5xx that borrows the code of a refusal is still a server in trouble.
    for (status, code) in [
        (500, "M_UNKNOWN"),
        (500, "M_FORBIDDEN"),
        (429, "M_LIMIT_EXCEEDED"),
    ] {
        let server = Server::start(two_rooms(status, code)).await;
        let provider = provider(&server).await;
        assert!(
            provider.fetch(None).await.is_err(),
            "{status} {code} fails the fetch"
        );
        assert!(
            provider.fetch(None).await.is_err(),
            "{status} {code}: nothing is given up"
        );
    }
}
