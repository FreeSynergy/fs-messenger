//! Fourth review, finding 1: an empty `/messages` chunk with an `end` does not end a gap (CS API
//! v1.16: "clients should continue to paginate until no end property is returned"). A server that
//! never stops sending empty pages is bounded, and the gap stays open. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fs_messenger_core::{Ingest, Provider};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{account, core, login, ordinary, plain, support, sync};
use serde_json::{Value, json};

async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(account(server), &Password::new("pw"), store)
        .await
        .unwrap()
}

/// The first `/sync` is limited and names the gap `p0`; later ones are empty.
fn syncs(counter: &AtomicUsize) -> Reply {
    match counter.fetch_add(1, Ordering::SeqCst) {
        0 => sync("s1", &[], true, Some("p0")),
        n => sync(&format!("s{}", n + 1), &[], false, None),
    }
}

#[tokio::test]
async fn an_empty_page_with_an_end_goes_on_to_the_next_page() {
    let counter = AtomicUsize::new(0);
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(&counter),
        p if p.contains("/messages") && p.contains("from=p0") => {
            Reply::json(200, &json!({"start":"p0","end":"p1","chunk":[]}))
        }
        p if p.contains("/messages") && p.contains("from=p1") => {
            Reply::json(200, &json!({"start":"p1","chunk":[plain("$old", "hi")]}))
        }
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let mut cursor = None;
    let mut ids = Vec::new();
    for _ in 0..5 {
        let batch = provider.fetch(cursor.as_ref()).await.unwrap();
        ids.extend(batch.messages.iter().map(|m| m.aliases[0].id.clone()));
        cursor = batch.next;
        if !batch.more {
            break;
        }
    }
    assert!(ids.iter().any(|i| i.contains("$old")), "lost: {ids:?}");
}

#[tokio::test]
async fn endless_empty_pages_end_in_time_and_the_gap_stays_open() {
    let (counter, pages) = (AtomicUsize::new(0), Arc::new(AtomicUsize::new(0)));
    let served = pages.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(&counter),
        p if p.contains("/messages") => {
            let i = served.fetch_add(1, Ordering::SeqCst);
            let end = format!("p{}", i + 1);
            Reply::json(200, &json!({"start":"x","end":end,"chunk":[]}))
        }
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let mut cursor = None;
    let mut seen = 0;
    for n in 0..3 {
        let fetch = tokio::time::timeout(Duration::from_secs(20), provider.fetch(cursor.as_ref()));
        let batch = fetch.await.expect("a fetch did not return").unwrap();
        let text = batch.next.as_ref().unwrap().0.clone();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed["gaps"].as_array().map(Vec::len),
            Some(1),
            "fetch {n}"
        );
        let now = pages.load(Ordering::SeqCst);
        assert!(now > seen, "fetch {n}: no progress");
        assert!(now - seen <= 20, "fetch {n}: {} empty pages", now - seen);
        seen = now;
        cursor = batch.next;
    }
}

#[tokio::test]
async fn sync_of_the_core_returns_on_endless_empty_pages() {
    let (counter, pages) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(&counter),
        p if p.contains("/messages") => {
            let end = format!("p{}", pages.fetch_add(1, Ordering::SeqCst) + 1);
            Reply::json(200, &json!({"start":"x","end":end,"chunk":[]}))
        }
        p => ordinary(p),
    })
    .await;
    let (repo, id) = core(provider(&server).await).await;
    for _ in 0..3 {
        let done = tokio::time::timeout(Duration::from_secs(20), repo.sync(id)).await;
        assert!(done.is_ok(), "sync() did not return");
    }
}
