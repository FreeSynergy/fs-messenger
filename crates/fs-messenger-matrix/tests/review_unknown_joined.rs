//! A room joined and left inside one lost answer: the own join is in the answer, so the room counts
//! as accepted, and it is read from its start once; after any failure, and when the replay sees it
//! under another heading. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fs_messenger_core::{FetchBatch, Provider, ProviderCursor};
use fs_messenger_matrix::MatrixProvider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, error, from_the_start, ids, is_room, login, messages_for, ordinary, own_event, page,
    plain, provider, room_id, timeline,
};
use serde_json::json;

/// Fetch 2 (`s1` to `s2`) brings `!b` joined and `!a` under `leave` with the own join, `$a1` and the
/// own leave. `b_broken` and `a_broken` make `/messages` of that room answer 500. With `rejoined`,
/// the second `/sync?since=s1` (our own, in the replay) shows `!a` under `join` instead.
fn server_for(
    b_broken: Arc<AtomicBool>,
    a_broken: Arc<AtomicBool>,
    rejoined: bool,
) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    let asked = Arc::new(AtomicUsize::new(0));
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s2") => answer("s3", &json!({})),
        p if p.contains("since=s3") || p.contains("since=s9") => answer("s9", &json!({})),
        p if p.contains("since=s1") => {
            let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
            let first = asked.fetch_add(1, Ordering::SeqCst) == 0;
            if rejoined && !first {
                let a = timeline(&[plain("$away", "said while away")], false, Some("pa2"));
                answer("s2", &json!({"join": {room_id("b"): b, room_id("a"): a}}))
            } else {
                let events = [
                    own_event("$aj", "join", Some("invite")),
                    plain("$a1", "said"),
                    own_event("$out", "leave", Some("join")),
                ];
                let a = timeline(&events, false, Some("pa"));
                answer(
                    "s2",
                    &json!({"join": {room_id("b"): b}, "leave": {room_id("a"): a}}),
                )
            }
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if p.contains("/messages") && is_room(p, "a") => {
            if a_broken.load(Ordering::SeqCst) {
                return error(500, "M_UNKNOWN");
            }
            if p.contains("to=s1") {
                return page(&[plain("$a1", "said")]);
            }
            page(&[plain("$a0", "before the join")])
        }
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

async fn start(rejoined: bool) -> Lost {
    let (b, a) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let server = Server::start(server_for(b.clone(), a.clone(), rejoined)).await;
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

/// Fetch (following `cursor`) until `$a1` and `$a0` have come, at most 8 times.
async fn until_both(
    p: &MatrixProvider,
    mut cursor: Option<ProviderCursor>,
) -> (Vec<String>, Option<ProviderCursor>) {
    let mut seen = Vec::new();
    for _ in 0..8 {
        let Ok(batch): Result<FetchBatch, _> = p.fetch(cursor.as_ref()).await else {
            continue;
        };
        seen.extend(ids(&batch));
        cursor = batch.next;
        if ["$a1", "$a0"]
            .iter()
            .all(|w| seen.iter().any(|i| i.contains(w)))
        {
            break;
        }
    }
    (seen, cursor)
}

#[tokio::test]
async fn left_unknown_then_joined_in_the_replay_reads_from_the_start() {
    let lost = start(true).await;
    lost.b.store(true, Ordering::SeqCst);
    lost.fails().await;
    lost.b.store(false, Ordering::SeqCst);
    let (seen, _) = until_both(&lost.provider, lost.cursor.clone()).await;
    for want in ["$away", "$a0"] {
        assert!(
            seen.iter().any(|i| i.contains(want)),
            "{want} lost: {seen:?}"
        );
    }
}

#[tokio::test]
async fn joined_and_left_in_a_lost_answer_after_any_failure() {
    for (b_fails, a_fails, keep) in [
        (1, 0, true),
        (2, 0, true),
        (3, 0, true),
        (0, 1, true),
        (0, 0, false),
    ] {
        let lost = start(false).await;
        if b_fails > 0 {
            lost.b.store(true, Ordering::SeqCst);
            for _ in 0..b_fails {
                lost.fails().await;
            }
            lost.b.store(false, Ordering::SeqCst);
        }
        if a_fails > 0 {
            lost.a.store(true, Ordering::SeqCst);
            lost.fails().await;
            lost.a.store(false, Ordering::SeqCst);
        }
        if !keep {
            let _dropped = lost.provider.fetch(lost.cursor.as_ref()).await.unwrap();
        }
        let (seen, cursor) = until_both(&lost.provider, lost.cursor.clone()).await;
        let case = format!("b {b_fails}, a {a_fails}, kept {keep}");
        for want in ["$a1", "$a0"] {
            assert!(
                seen.iter().any(|i| i.contains(want)),
                "{case}: {want} lost: {seen:?}"
            );
        }
        // A fetch that went through has the room as away in the cursor: no read from its start now.
        let before = from_the_start(&messages_for(&lost.server, "a")).len();
        let mut cursor = cursor;
        for _ in 0..3 {
            cursor = lost.provider.fetch(cursor.as_ref()).await.unwrap().next;
        }
        let after = from_the_start(&messages_for(&lost.server, "a")).len();
        assert_eq!(before, after, "{case}: read from the start again");
    }
}
