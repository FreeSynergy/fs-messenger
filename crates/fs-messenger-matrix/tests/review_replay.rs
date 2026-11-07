//! A lost answer is asked for again from the cursor, by our own `/sync`, not guessed from what the
//! SDK holds. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fs_messenger_core::Provider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, cursor_json, error, ids, is_sdk_sync, login, ordinary, page, plain, provider, room_id,
    syncs_after, timeline,
};
use serde_json::json;

fn joined_b(broken: Arc<AtomicBool>) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s2") => answer("s3", &json!({})),
        p if p.contains("since=s3") || p.contains("since=s9") => answer("s9", &json!({})),
        p if p.contains("since=s1") => {
            let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
            answer("s2", &json!({"join": {room_id("b"): b}}))
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if p.contains("/messages") && broken.load(Ordering::SeqCst) => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => page(&[plain("$b0", "before")]),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn a_replay_asks_sync_from_the_cursor() {
    let broken = Arc::new(AtomicBool::new(true));
    let server = Server::start(joined_b(broken.clone())).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    assert!(
        provider.fetch(one.next.as_ref()).await.is_err(),
        "fetch 2 fails"
    );
    broken.store(false, Ordering::SeqCst);

    let before = server.heard().len();
    let three = provider.fetch(one.next.as_ref()).await.unwrap();
    let syncs = syncs_after(&server, before);
    assert_eq!(syncs.len(), 1, "{syncs:?}");
    assert!(
        syncs[0].contains("since=s1") && !is_sdk_sync(&syncs[0]),
        "{syncs:?}"
    );
    assert!(
        three.more,
        "the ordinary fetch from the SDK's position follows at once"
    );
    assert_eq!(cursor_json(&three)["since"], "s2");
    assert!(
        ids(&three).iter().any(|i| i.contains("$b0")),
        "{:?}",
        ids(&three)
    );

    let before = server.heard().len();
    let four = provider.fetch(three.next.as_ref()).await.unwrap();
    let syncs = syncs_after(&server, before);
    assert!(
        syncs.len() == 1 && syncs[0].contains("since=s2") && is_sdk_sync(&syncs[0]),
        "{syncs:?}"
    );
    assert!(!four.more);
    assert_eq!(cursor_json(&four)["since"], "s3");
}

#[tokio::test]
async fn an_old_since_rejected_fails_visibly() {
    let asked = Arc::new(AtomicUsize::new(0));
    let broken = Arc::new(AtomicBool::new(true));
    let flag = broken.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s1") => {
            if asked.fetch_add(1, Ordering::SeqCst) == 0 {
                let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
                answer("s2", &json!({"join": {room_id("b"): b}}))
            } else {
                error(400, "M_UNKNOWN")
            }
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if p.contains("/messages") && flag.load(Ordering::SeqCst) => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => page(&[plain("$b0", "before")]),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    assert!(
        provider.fetch(one.next.as_ref()).await.is_err(),
        "fetch 2 fails"
    );
    broken.store(false, Ordering::SeqCst);
    // Everything would work now; only the server's refusal of the old token holds the account up,
    // and that is an error, not a silent skip of the stretch.
    let refused = provider.fetch(one.next.as_ref()).await;
    let text = format!("{refused:?}");
    assert!(refused.is_err(), "the account must be seen to be held up");
    assert!(!text.contains(review_common::TOKEN), "{text}");
}
