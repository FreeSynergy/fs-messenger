//! Third review, finding 2: a server whose every `/sync` is over the timeline limit and whose
//! `/messages` pages are small with ever-new tokens must neither keep `sync()` busy for ever nor
//! make the cursor grow without end. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fs_messenger_core::{Ingest, Provider};
use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{ROOM, account, core, login, ordinary, plain, support, sync};
use serde_json::{Value, json};

/// Every `/sync` brings 3 new events for a limit of 1; every `/messages` page brings one event
/// and a token never seen before.
async fn hostile() -> (Server, Arc<AtomicUsize>) {
    let (syncs, pages) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let served = pages.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => {
            let n = syncs.fetch_add(1, Ordering::SeqCst);
            let events: Vec<Value> = (0..3).map(|i| plain(&format!("$s{n}-{i}"), "hi")).collect();
            sync(
                &format!("s{}", n + 1),
                &events,
                false,
                Some(&format!("pb{n}")),
            )
        }
        p if p.contains("/messages") => {
            let i = served.fetch_add(1, Ordering::SeqCst);
            let chunk = [plain(&format!("$m{i}"), "hi")];
            let body = json!({"start":"x","end":format!("p{}", i + 1),"chunk":chunk});
            Reply::json(200, &body)
        }
        p => ordinary(p),
    })
    .await;
    (server, pages)
}

fn limited(server: &Server) -> MatrixAccount {
    let mut account = account(server);
    account.timeline_limit = 1;
    account
}

#[tokio::test]
async fn many_fetches_keep_the_gaps_and_the_cursor_small() {
    let (server, _) = hostile().await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(limited(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let mut cursor = None;
    for n in 0..40 {
        let batch = tokio::time::timeout(Duration::from_secs(30), provider.fetch(cursor.as_ref()))
            .await
            .expect("a fetch did not return")
            .unwrap();
        let text = batch.next.as_ref().unwrap().0.clone();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        let open = parsed["gaps"].as_array().map_or(0, Vec::len);
        assert!(open <= 8, "fetch {n}: {open} open gaps for {ROOM}");
        assert!(
            text.len() < 4000,
            "fetch {n}: cursor of {} bytes",
            text.len()
        );
        cursor = batch.next;
    }
}

#[tokio::test]
async fn sync_of_the_core_returns() {
    let (server, pages) = hostile().await;
    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(limited(&server), &Password::new("pw"), store)
        .await
        .unwrap();
    let (repo, id) = core(provider).await;
    for _ in 0..3 {
        let done = tokio::time::timeout(Duration::from_secs(30), repo.sync(id)).await;
        assert!(
            done.is_ok(),
            "sync() did not return; {} pages",
            pages.load(Ordering::SeqCst)
        );
    }
}
