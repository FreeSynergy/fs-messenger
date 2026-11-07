//! The limits of the check: a flood of invitations, and a knock. A raw server, not Matrix.

mod review_common;

use fs_messenger_core::Provider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, cursor_json, ids, is_check, is_room, login, messages_for, ordinary, page, plain,
    provider, room_id, timeline,
};
use serde_json::json;

fn nothing() -> Reply {
    page(&[])
}

#[tokio::test]
async fn checks_are_bounded_under_an_invitation_flood() {
    let server = Server::start(|path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s1") => {
            let rooms: serde_json::Map<_, _> = (0..1000)
                .map(|i| {
                    (
                        room_id(&format!("f{i}")),
                        json!({"invite_state": {"events": []}}),
                    )
                })
                .collect();
            answer("s2", &json!({"invite": rooms}))
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if p.contains("/messages") => nothing(),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    let asked = server
        .heard()
        .iter()
        .filter(|(p, _)| p.contains("/messages"))
        .count();
    assert!(asked <= 20, "{asked} requests in one fetch");
    let cursor = cursor_json(&two);
    let checks = cursor["gaps"].as_array().map_or(0, Vec::len);
    assert!(checks <= 64, "{checks} checks kept");
    let named = cursor["given_up"].as_array().unwrap();
    assert!(
        !named.is_empty() && named.iter().all(|g| g["why"] == "overflow"),
        "{named:?}"
    );
    assert!(named.len() <= 32);
}

#[tokio::test]
async fn knock_unknown_and_known() {
    let server = Server::start(|path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s1") => {
            let knock = |n: &str| (room_id(n), json!({"knock_state": {"events": []}}));
            let rooms: serde_json::Map<_, _> = [knock("u"), knock("k")].into_iter().collect();
            answer("s2", &json!({"knock": rooms}))
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$k1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("k"): t}}))
        }
        p if is_check(p) => nothing(),
        p if p.contains("/messages") && is_room(p, "k") => page(&[plain("$k2", "said")]),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    let unknown = messages_for(&server, "u");
    assert!(
        !unknown.is_empty() && unknown.iter().all(|p| is_check(p)),
        "{unknown:?}"
    );
    let known = messages_for(&server, "k");
    assert!(
        known
            .iter()
            .any(|p| p.contains("from=s2") && p.contains("to=s1")),
        "{known:?}"
    );
    assert!(ids(&two).iter().any(|i| i.contains("$k2")));
    let cursor = cursor_json(&two);
    assert_eq!(
        cursor["away"],
        json!([{"room": room_id("k"), "at": "s1"}]),
        "{cursor}"
    );
    assert!(!cursor.to_string().contains(&room_id("u")), "{cursor}");
}
