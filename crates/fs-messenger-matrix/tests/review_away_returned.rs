//! Ninth review, finding 1: a room the account accepted once and left (away) is not asked about
//! when it comes back as an invitation, a knock or a leave without an own join: the account agreed to
//! this room already, so the stretch since the leave is read at once. A check of the account's own
//! membership events cannot see a return that the server hides (the return, a message and a kick
//! between two syncs), and the message would be lost. A raw server, not Matrix; the real server is in
//! `tests/server/membership_away.rs`.

mod review_common;

use fs_messenger_core::Provider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, cursor_json, ids, is_check, is_room, login, ordinary, own_membership, page, plain,
    provider, room_id, timeline,
};
use serde_json::{Value, json};

/// `!a` is read at `s1`, left in `s2` (away at `s1`). Fetch 3 (`since=s2`) brings `third`. A check
/// shows only the old leave (nothing to the point), a read down to `s1` shows `$wb`.
fn server_for(third: Value) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s3") => answer("s9", &json!({})),
        p if p.contains("since=s2") => answer("s3", &third),
        p if p.contains("since=s1") => {
            let t = timeline(&[own_membership("$out", "leave")], false, Some("pl"));
            answer("s2", &json!({"leave": {room_id("a"): t}}))
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$a1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("a"): t}}))
        }
        p if is_check(p) => page(&[]),
        p if p.contains("/messages") && is_room(p, "a") && p.contains("to=s1") => {
            page(&[plain("$wb", "said while back")])
        }
        p => ordinary(p),
    }
}

/// Three fetches against `third`; the ids of the third and the asked `/messages` paths.
async fn third_fetch(third: Value) -> (Vec<String>, Vec<String>, Value) {
    let server = Server::start(server_for(third)).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    let three = provider.fetch(two.next.as_ref()).await.unwrap();
    let asked = review_common::messages_for(&server, "a");
    (ids(&three), asked, cursor_json(&three))
}

#[tokio::test]
async fn an_invitation_or_knock_to_a_room_left_before_reads_the_stretch_at_once() {
    for how in ["invite", "knock"] {
        let state = if how == "invite" {
            "invite_state"
        } else {
            "knock_state"
        };
        let third = json!({how: {room_id("a"): {state: {"events": []}}}});
        let (got, asked, cursor) = third_fetch(third).await;
        assert!(
            got.iter().any(|i| i.contains("$wb")),
            "{how}: lost {got:?} {asked:?}"
        );
        assert!(
            asked.iter().all(|p| !is_check(p)),
            "{how}: a check was asked {asked:?}"
        );
        assert_eq!(
            cursor["away"],
            json!([{"room": room_id("a"), "at": "s1"}]),
            "{cursor}"
        );
        assert_eq!(cursor.get("given_up"), None, "{cursor}");
    }
}

#[tokio::test]
async fn a_leave_without_an_own_join_in_a_room_left_before_reads_the_stretch_at_once() {
    let kick = json!({"type":"m.room.member","event_id":"$kick","sender":"@carol:x.test",
        "origin_server_ts":950,"state_key":review_common::USER,"content":{"membership":"leave"}});
    let t = timeline(&[kick], true, Some("pk"));
    let (got, asked, cursor) = third_fetch(json!({"leave": {room_id("a"): t}})).await;
    assert!(
        got.iter().any(|i| i.contains("$wb")),
        "lost {got:?} {asked:?}"
    );
    assert!(
        asked.iter().all(|p| !is_check(p)),
        "a check was asked {asked:?}"
    );
    assert_eq!(
        cursor["away"],
        json!([{"room": room_id("a"), "at": "s1"}]),
        "{cursor}"
    );
}
