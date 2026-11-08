//! Second review, findings 3 and 4: one fetch reads a bounded number of pages and events, whatever a
//! server answers, and what it leaves is kept, not lost. A raw server, not Matrix.

mod review_common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fs_messenger_core::{Ingest, Provider, ProviderCursor};
use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{account, core, login, ordinary, plain, support, sync};
use serde_json::{Value, json};

/// The first `/sync` is limited and names a gap; later ones are empty.
fn syncs(counter: &Arc<AtomicUsize>, first: &[Value], limited: bool) -> Reply {
    match counter.fetch_add(1, Ordering::SeqCst) {
        0 => sync("s1", first, limited, Some("p0")),
        n => sync(&format!("s{}", n + 1), &[], false, None),
    }
}

async fn provider(account: MatrixAccount) -> MatrixProvider {
    let store = support::fresh_store().await;
    let password = Password::new("pw");
    MatrixProvider::login(account, &password, store)
        .await
        .unwrap()
}

/// Fetch from `cursor` until `more` ends. The ids delivered, and the size of each fetch.
async fn drain(
    provider: &MatrixProvider,
    mut cursor: Option<ProviderCursor>,
) -> (HashSet<String>, Vec<usize>) {
    let (mut ids, mut sizes) = (HashSet::new(), Vec::new());
    for _ in 0..40 {
        let batch = provider.fetch(cursor.as_ref()).await.unwrap();
        sizes.push(batch.messages.len());
        ids.extend(batch.messages.iter().map(|m| m.aliases[0].id.clone()));
        cursor = batch.next;
        if !batch.more {
            break;
        }
    }
    (ids, sizes)
}

#[tokio::test]
async fn endless_pages_end_one_sync_and_the_next_goes_on() {
    let (syncs_seen, pages) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (counter, page_count) = (syncs_seen.clone(), pages.clone());
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(&counter, &[], true),
        p if p.contains("/messages") => {
            let i = page_count.fetch_add(1, Ordering::SeqCst);
            let chunk = [plain(&format!("$m{i}"), "hi")];
            Reply::json(
                200,
                &json!({"start":"x","end":format!("p{}", i + 1),"chunk":chunk}),
            )
        }
        p => ordinary(p),
    })
    .await;
    let (repo, id) = core(provider(account(&server)).await).await;

    let first = tokio::time::timeout(Duration::from_secs(20), repo.sync(id)).await;
    assert!(
        first.is_ok(),
        "sync() did not return; {} pages",
        pages.load(Ordering::SeqCst)
    );
    let after_first = pages.load(Ordering::SeqCst);
    assert!(
        after_first <= 40,
        "too many pages in one sync: {after_first}"
    );

    let second = tokio::time::timeout(Duration::from_secs(20), repo.sync(id)).await;
    assert!(second.is_ok());
    assert!(
        pages.load(Ordering::SeqCst) > after_first,
        "the gap was lost: nothing went on"
    );
}

#[tokio::test]
async fn a_page_of_3000_is_cut_and_the_rest_comes_in_the_next_fetch() {
    let counter = Arc::new(AtomicUsize::new(0));
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => syncs(&counter, &[], true),
        p if p.contains("/messages") => {
            let chunk: Vec<_> = (0..3000)
                .map(|i| plain(&format!("$big{i}"), "hi"))
                .collect();
            Reply::json(200, &json!({"start":"x","chunk":chunk}))
        }
        p => ordinary(p),
    })
    .await;
    let provider = provider(account(&server)).await;
    let (ids, sizes) = drain(&provider, None).await;
    assert!(
        sizes.iter().all(|n| *n <= 500),
        "a fetch over the limit: {sizes:?}"
    );
    assert_eq!(ids.len(), 3000, "events lost; per fetch: {sizes:?}");
}

#[tokio::test]
async fn a_sync_over_the_timeline_limit_is_cut_and_read_again_through_messages() {
    let counter = Arc::new(AtomicUsize::new(0));
    let events: Vec<Value> = (0..300).map(|i| plain(&format!("$e{i}"), "hi")).collect();
    let newest_first: Vec<Value> = events.iter().rev().cloned().collect();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => match counter.fetch_add(1, Ordering::SeqCst) {
            0 => sync("s1", &events, false, None),
            n => sync(&format!("s{}", n + 1), &[], false, None),
        },
        p if p.contains("/messages") => {
            Reply::json(200, &json!({"start":"x","chunk":newest_first}))
        }
        p => ordinary(p),
    })
    .await;
    let mut account = account(&server);
    account.timeline_limit = 50;
    let provider = provider(account).await;

    let first = provider.fetch(None).await.unwrap();
    assert!(
        first.messages.len() <= 50,
        "{} over the limit of 50",
        first.messages.len()
    );
    assert!(first.more, "the rest is not forgotten");
    let (rest, _) = drain(&provider, first.next).await;
    let delivered: HashSet<String> = first
        .messages
        .iter()
        .map(|m| m.aliases[0].id.clone())
        .collect();
    assert_eq!(delivered.union(&rest).count(), 300, "events lost");
}
