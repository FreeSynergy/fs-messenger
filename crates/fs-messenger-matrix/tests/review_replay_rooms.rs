//! A replay changes only what the stretch changed: left rooms it did not touch are not read, and an
//! answer seen twice moves no room twice. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fs_messenger_core::Provider;
use review_common::support::server::Server;
use review_common::{
    answer, cursor_json, error, ids, is_room, login, ordinary, own_event, page, plain, provider,
    room_id, timeline,
};
use serde_json::json;

#[tokio::test]
async fn an_unchanged_left_room_is_not_read_by_a_replay() {
    let broken = Arc::new(AtomicBool::new(true));
    let flag = broken.clone();
    let server = Server::start(move |path| {
        let rooms = |section: &str, event: serde_json::Value, prev: Option<&str>| {
            let all: serde_json::Map<_, _> = (0..30)
                .map(|i| {
                    (
                        room_id(&format!("c{i}")),
                        timeline(std::slice::from_ref(&event), false, prev),
                    )
                })
                .collect();
            json!({ section: all })
        };
        match path {
            p if p.ends_with("/login") => login(),
            p if p.contains("since=s2") => answer("s3", &json!({})),
            p if p.contains("since=s1") => {
                let mut rooms = rooms(
                    "leave",
                    own_event("$out", "leave", Some("join")),
                    Some("pc"),
                );
                let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
                rooms["join"] = json!({room_id("b"): b});
                answer("s2", &rooms)
            }
            p if p.contains("/sync") => answer("s1", &rooms("join", plain("$c1", "hi"), None)),
            p if p.contains("/messages") && is_room(p, "b") && flag.load(Ordering::SeqCst) => {
                error(500, "M_UNKNOWN")
            }
            p if p.contains("/messages") && is_room(p, "b") => page(&[plain("$b0", "before")]),
            // The first reading of each room (fetch 1) is an ordinary one.
            p if p.contains("/messages") => page(&[]),
            p => ordinary(p),
        }
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let after_first = server.heard().len();
    assert!(
        provider.fetch(one.next.as_ref()).await.is_err(),
        "fetch 2 fails"
    );
    broken.store(false, Ordering::SeqCst);
    let three = provider.fetch(one.next.as_ref()).await.unwrap();
    assert!(ids(&three).iter().any(|i| i.contains("$b0")));
    let read: Vec<_> = server
        .heard()
        .into_iter()
        .skip(after_first)
        .map(|(p, _)| p)
        .filter(|p| p.contains("/messages") && !is_room(p, "b"))
        .collect();
    assert!(
        read.is_empty(),
        "left rooms the answers did not touch were read: {read:?}"
    );
}

#[tokio::test]
async fn an_overlapping_replay_changes_no_state_twice() {
    let seen = Arc::new(AtomicUsize::new(0));
    let broken = Arc::new(AtomicBool::new(true));
    let flag = broken.clone();
    let server = Server::start(move |path| {
        let kick = || {
            timeline(
                &[own_event("$kick", "leave", Some("join"))],
                false,
                Some("pk"),
            )
        };
        match path {
            p if p.ends_with("/login") => login(),
            p if p.contains("since=s2") => answer("s3", &json!({"leave": {room_id("a"): kick()}})),
            p if p.contains("since=s3") => answer("s4", &json!({})),
            p if p.contains("since=s1") => {
                seen.fetch_add(1, Ordering::SeqCst);
                let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
                let rooms = json!({"join": {room_id("b"): b}, "leave": {room_id("a"): kick()}});
                answer("s2", &rooms)
            }
            p if p.contains("/sync") => {
                let t = timeline(&[plain("$a1", "hi")], false, None);
                answer("s1", &json!({"join": {room_id("a"): t}}))
            }
            p if p.contains("/messages") && is_room(p, "b") && flag.load(Ordering::SeqCst) => {
                error(500, "M_UNKNOWN")
            }
            p if p.contains("/messages") && is_room(p, "b") => page(&[plain("$b0", "before")]),
            p if p.contains("/messages") => page(&[]),
            p => ordinary(p),
        }
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    assert!(
        provider.fetch(one.next.as_ref()).await.is_err(),
        "fetch 2 fails"
    );
    broken.store(false, Ordering::SeqCst);
    let three = provider.fetch(one.next.as_ref()).await.unwrap();
    let four = provider.fetch(three.next.as_ref()).await.unwrap();
    let cursor = cursor_json(&four);
    assert_eq!(
        cursor["away"],
        json!([{"room": room_id("a"), "at": "s1"}]),
        "{cursor}"
    );
    let starts = review_common::messages_for(&server, "a");
    assert!(
        review_common::from_the_start(&starts).is_empty(),
        "{starts:?}"
    );
}
