//! Seventh review, finding 1: a room the account joined and left within one `/sync` answer, in an
//! answer the SDK took and the fetch then lost (any failure after the SDK, one or several), must
//! still bring what it said. The answer shows the own join (without it the room would not count as
//! accepted, see `review_checks`). After a fetch that went through, the room is not read from its
//! start again. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fs_messenger_core::{Provider, ProviderCursor};
use fs_messenger_matrix::MatrixProvider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, error, ids, is_room, login, ordinary, own_event, page, plain, provider, room_id,
    timeline,
};
use serde_json::json;

/// Fetch 1 (`s1`) knows nothing. Fetch 2 (`s1` to `s2`) brings `!b` joined, and `!a` only under
/// `leave`: joined and left in that stretch, `$a1` and the leave in its timeline. While `b_broken`
/// is set `/messages` of `!b` answers 500 (the SDK retries a 500 by itself, so a flag, not a count);
/// the same for the reading of `!a` from its start (`from=pa`) and `a_broken`.
fn server_for(
    b_broken: Arc<AtomicBool>,
    a_broken: Arc<AtomicBool>,
) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s2") => answer("s3", &json!({})),
        p if p.contains("since=s3") || p.contains("since=s9") => answer("s9", &json!({})),
        p if p.contains("since=s1") => {
            let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
            let events = [
                own_event("$aj", "join", Some("invite")),
                plain("$a1", "said"),
                own_event("$out", "leave", Some("join")),
            ];
            let a = timeline(&events, false, Some("pa"));
            let rooms = json!({"join": {room_id("b"): b}, "leave": {room_id("a"): a}});
            answer("s2", &rooms)
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if p.contains("/messages") && is_room(p, "a") && p.contains("from=pa") => {
            if a_broken.load(Ordering::SeqCst) {
                return error(500, "M_UNKNOWN");
            }
            page(&[plain("$a0", "before the join")])
        }
        p if p.contains("/messages") && is_room(p, "a") && p.contains("to=s1") => {
            if a_broken.load(Ordering::SeqCst) {
                return error(500, "M_UNKNOWN");
            }
            page(&[plain("$a1", "said")])
        }
        p if p.contains("/messages") && is_room(p, "a") => page(&[plain("$a0", "from the start")]),
        p if p.contains("/messages") && b_broken.load(Ordering::SeqCst) => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => page(&[plain("$b1", "hi")]),
        p => ordinary(p),
    }
}

struct Lost {
    server: Server,
    provider: MatrixProvider,
    b: Arc<AtomicBool>,
    a: Arc<AtomicBool>,
    cursor: Option<ProviderCursor>,
}

/// Fetch 1 is done; both flags are off.
async fn start() -> Lost {
    let (b, a) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let server = Server::start(server_for(b.clone(), a.clone())).await;
    let provider = provider(&server).await;
    let cursor = provider.fetch(None).await.unwrap().next;
    Lost {
        server,
        provider,
        b,
        a,
        cursor,
    }
}

impl Lost {
    async fn fails(&self) {
        let result = self.provider.fetch(self.cursor.as_ref()).await;
        assert!(result.is_err(), "this fetch was to fail");
    }
}

/// Fetch until `$a1` has come, at most 6 times. Failing fetches are counted, not unwrapped.
async fn until_a1(
    provider: &MatrixProvider,
    mut cursor: Option<ProviderCursor>,
) -> (Vec<String>, usize) {
    let (mut seen, mut failed) = (Vec::new(), 0);
    for _ in 0..6 {
        match provider.fetch(cursor.as_ref()).await {
            Ok(batch) => {
                seen.extend(ids(&batch));
                cursor = batch.next;
            }
            Err(_) => failed += 1,
        }
        if seen.iter().any(|i| i.contains("$a1")) {
            break;
        }
    }
    (seen, failed)
}

#[tokio::test]
async fn joined_and_left_in_one_answer_then_one_failed_fetch_loses_nothing() {
    let lost = start().await;
    lost.b.store(true, Ordering::SeqCst);
    lost.fails().await;
    lost.b.store(false, Ordering::SeqCst);
    let (seen, _) = until_a1(&lost.provider, lost.cursor.clone()).await;
    assert!(seen.iter().any(|i| i.contains("$a1")), "lost: {seen:?}");
}

#[tokio::test]
async fn two_or_three_failed_fetches_in_a_row_lose_nothing() {
    for failures in [2, 3] {
        let lost = start().await;
        lost.b.store(true, Ordering::SeqCst);
        for _ in 0..failures {
            lost.fails().await;
        }
        lost.b.store(false, Ordering::SeqCst);
        let (seen, _) = until_a1(&lost.provider, lost.cursor.clone()).await;
        assert!(
            seen.iter().any(|i| i.contains("$a1")),
            "{failures} failures, lost: {seen:?}"
        );
    }
}

#[tokio::test]
async fn a_cursor_the_core_did_not_keep_loses_nothing() {
    // No failure at all: fetch 2 works, but its batch and cursor never reach the core's store.
    let lost = start().await;
    let _dropped = lost.provider.fetch(lost.cursor.as_ref()).await.unwrap();
    let (seen, _) = until_a1(&lost.provider, lost.cursor.clone()).await;
    assert!(seen.iter().any(|i| i.contains("$a1")), "lost: {seen:?}");
}

#[tokio::test]
async fn a_failure_while_replaying_the_left_room_itself_loses_nothing() {
    // `!b` fails once; then the replay of `!a` fails once; then all goes through.
    let lost = start().await;
    lost.b.store(true, Ordering::SeqCst);
    lost.fails().await;
    lost.b.store(false, Ordering::SeqCst);
    lost.a.store(true, Ordering::SeqCst);
    lost.fails().await;
    lost.a.store(false, Ordering::SeqCst);
    let (seen, _) = until_a1(&lost.provider, lost.cursor.clone()).await;
    assert!(seen.iter().any(|i| i.contains("$a1")), "lost: {seen:?}");
}

#[tokio::test]
async fn a_left_room_is_met_from_its_start_only_in_failed_attempts_never_after_a_successful_one() {
    let lost = start().await;
    lost.b.store(true, Ordering::SeqCst);
    for _ in 0..3 {
        let _ = lost.provider.fetch(lost.cursor.as_ref()).await;
    }
    lost.b.store(false, Ordering::SeqCst);
    // Failed attempts each began the room again (`from=pa`); the first fetch that goes through, with
    // its cursor kept, reads it once more and no later one does.
    let mut seen = Vec::new();
    let first = lost.provider.fetch(lost.cursor.as_ref()).await.unwrap();
    seen.extend(ids(&first));
    let mut cursor = first.next;
    let reads = |lost: &Lost| {
        let asked = review_common::messages_for(&lost.server, "a");
        asked.iter().filter(|p| p.contains("from=pa")).count()
    };
    let after_first = reads(&lost);
    let open = |lost: &Lost| {
        let asked = review_common::messages_for(&lost.server, "a");
        review_common::from_the_start(&asked).len()
    };
    let open_after_first = open(&lost);
    for _ in 0..3 {
        let batch = lost.provider.fetch(cursor.as_ref()).await.unwrap();
        seen.extend(ids(&batch));
        cursor = batch.next;
    }
    assert_eq!(
        reads(&lost),
        after_first,
        "read from the start after a fetch that went through"
    );
    // Not only `from=pa`: no `/messages` of the room without an end (`to=`) after that fetch, from
    // whatever token and by whatever name.
    assert_eq!(
        open(&lost),
        open_after_first,
        "an unbounded read after a fetch that went through"
    );
    let got = |w: &str| seen.iter().any(|i| i.contains(w));
    assert!(got("$a1") && got("$a0"), "lost: {seen:?}");
}
