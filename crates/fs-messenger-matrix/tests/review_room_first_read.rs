//! Fourth review, finding 2: a room counts as read only when its first reading went through. If a
//! fetch fails (or the core refuses its cursor) after the SDK took the answer, the next fetch must
//! still read what the room said before the join. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fs_messenger_core::{Provider, ProviderCursor};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{ROOM, account, login, ordinary, plain, support};
use serde_json::{Value, json};

/// Sync 1 knows no room; sync 2 (`since=s1`) brings the room joined, with `$new` and the token
/// `pj`; later ones are empty. `/messages` is whatever the test says.
fn syncs(path: &str) -> Reply {
    let next = |n: &str, rooms: Value| Reply::json(200, &json!({"next_batch": n, "rooms": rooms}));
    if path.contains("since=s2") {
        next("s3", json!({}))
    } else if path.contains("since=s1") {
        let timeline = json!({"events":[plain("$new","hi")],"limited":false,"prev_batch":"pj"});
        next("s2", json!({"join": {ROOM: {"timeline": timeline}}}))
    } else if path.contains("since=") {
        next("s4", json!({}))
    } else {
        next("s1", json!({}))
    }
}

/// What the room said before the join: only a read that is not bounded by `to=` finds it.
fn before_the_join(path: &str) -> Reply {
    let chunk = if path.contains("&to=") || path.contains("?to=") {
        vec![]
    } else {
        vec![plain("$old", "hi")]
    };
    Reply::json(200, &json!({"start":"x","chunk":chunk}))
}

async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(account(server), &Password::new("pw"), store)
        .await
        .unwrap()
}

fn ids(batch: &fs_messenger_core::FetchBatch) -> Vec<String> {
    batch
        .messages
        .iter()
        .map(|m| m.aliases[0].id.clone())
        .collect()
}

/// Fetch 1 works, fetch 2 fails with `fail`, fetch 3 must bring `$old`.
async fn failed_then_ok(server: Server, broken: Arc<AtomicBool>) {
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    assert!(
        provider.fetch(one.next.as_ref()).await.is_err(),
        "fetch 2 should fail"
    );
    broken.store(false, Ordering::SeqCst);
    let mut cursor = one.next;
    let mut seen = Vec::new();
    for _ in 0..5 {
        let batch = provider.fetch(cursor.as_ref()).await.unwrap();
        seen.extend(ids(&batch));
        cursor = batch.next;
        if !batch.more {
            break;
        }
    }
    assert!(seen.iter().any(|i| i.contains("$old")), "lost: {seen:?}");
}

#[tokio::test]
async fn a_failed_first_reading_is_tried_again() {
    let broken = Arc::new(AtomicBool::new(true));
    let failed = broken.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(p),
        p if p.contains("/messages") && failed.load(Ordering::SeqCst) => {
            Reply::json(500, &json!({"errcode":"M_UNKNOWN","error":"boom"}))
        }
        p if p.contains("/messages") => before_the_join(p),
        p => ordinary(p),
    })
    .await;
    failed_then_ok(server, broken).await;
}

#[tokio::test]
async fn a_failure_on_a_later_page_of_the_first_reading_is_tried_again() {
    let broken = Arc::new(AtomicBool::new(true));
    let failed = broken.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(p),
        p if p.contains("/messages") && p.contains("from=pj") => {
            let body = json!({"start":"pj","end":"pk","chunk":[plain("$mid","hi")]});
            Reply::json(200, &body)
        }
        p if p.contains("/messages") && p.contains("from=pk") && failed.load(Ordering::SeqCst) => {
            Reply::json(500, &json!({"errcode":"M_UNKNOWN","error":"boom"}))
        }
        p if p.contains("/messages") => before_the_join(p),
        p => ordinary(p),
    })
    .await;
    failed_then_ok(server, broken).await;
}

#[tokio::test]
async fn a_cursor_the_core_did_not_keep_does_not_mark_the_room_read() {
    let pages = AtomicUsize::new(0);
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(p),
        p if p.contains("/messages") && pages.fetch_add(1, Ordering::SeqCst) == 0 => {
            Reply::json(200, &json!({"start":"pj","chunk":[plain("$mid","hi")]}))
        }
        p if p.contains("/messages") => before_the_join(p),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    // Fetch 2 works, but the core refuses its cursor: it keeps `one.next`.
    let _refused: Option<ProviderCursor> = provider.fetch(one.next.as_ref()).await.unwrap().next;
    let three = provider.fetch(one.next.as_ref()).await.unwrap();
    assert!(
        ids(&three).iter().any(|i| i.contains("$old")),
        "lost: {:?}",
        ids(&three)
    );
}
