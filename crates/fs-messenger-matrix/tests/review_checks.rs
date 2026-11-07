//! A room the cursor never knew as accepted is read only if the account is shown to have joined it
//! (DESIGN.md, "Room states"); otherwise one question to the server, about the account's own
//! membership events, and nothing else. A raw server, not Matrix.

mod review_common;

use fs_messenger_core::Provider;
use review_common::support::server::{Reply, Server};
use review_common::{
    USER, answer, cursor_json, error, from_the_start, ids, is_check, is_room, login, messages_for,
    ordinary, own_event, page, plain, provider, room_id, timeline,
};
use serde_json::{Value, json};

/// The account is invited (`how`) to `!a` at `s2`; `!a` was never known. The check is answered by
/// `check`, a read by `$a1`.
async fn invited(how: &'static str, check: fn() -> Reply) -> Server {
    Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s1") => {
            let state = if how == "invite" {
                "invite_state"
            } else {
                "knock_state"
            };
            answer("s2", &json!({how: {room_id("a"): {state: {"events": []}}}}))
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if is_check(p) => check(),
        p if p.contains("/messages") && is_room(p, "a") => page(&[plain("$a1", "said")]),
        p => ordinary(p),
    })
    .await
}

fn own_join() -> Reply {
    page(&[own_event("$aj", "join", Some("invite"))])
}

fn own_leave_after_a_join() -> Reply {
    page(&[own_event("$al", "leave", Some("join"))])
}

fn nothing() -> Reply {
    page(&[])
}

fn refused() -> Reply {
    error(403, "M_FORBIDDEN")
}

#[tokio::test]
async fn kicked_and_invited_again_unknown_room_is_read() {
    for found in [own_join as fn() -> Reply, own_leave_after_a_join] {
        let server = invited("invite", found).await;
        let provider = provider(&server).await;
        let one = provider.fetch(None).await.unwrap();
        let two = provider.fetch(one.next.as_ref()).await.unwrap();
        assert!(
            ids(&two).iter().any(|i| i.contains("$a1")),
            "lost: {:?}",
            ids(&two)
        );
        let cursor = cursor_json(&two);
        assert_eq!(
            cursor["away"],
            json!([{"room": room_id("a"), "at": "s1"}]),
            "{cursor}"
        );
        assert_eq!(cursor.get("gaps"), None, "{cursor}");
    }
}

#[tokio::test]
async fn an_invitation_without_an_own_join_reads_nothing() {
    let server = invited("invite", nothing).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    let asked = messages_for(&server, "a");
    assert!(
        !asked.is_empty() && asked.iter().all(|p| is_check(p)),
        "{asked:?}"
    );
    assert!(ids(&two).is_empty());
    let cursor = cursor_json(&two).to_string();
    assert!(!cursor.contains("!a:x.test"), "{cursor}");
}

/// `!a` was never known; the account declined the invitation (or is shown a join someone else sent).
/// The room is world readable, so its spam is in the timeline. `state` is what the answer's state says.
async fn declined(state: Value, limited: bool) -> (Server, Vec<String>) {
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s1") => {
            let events = [
                plain("$spam", "buy now"),
                own_event("$out", "leave", Some("invite")),
            ];
            let mut a = timeline(&events, limited, Some("pa"));
            a["state"] = json!({"events": [state.clone()]});
            answer("s2", &json!({"leave": {room_id("a"): a}}))
        }
        p if p.contains("/sync") => answer("s1", &json!({})),
        p if is_check(p) => nothing(),
        p if p.contains("/messages") => page(&[plain("$spam", "buy now")]),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    (server, ids(&two))
}

fn invite_state() -> Value {
    json!({"type":"m.room.member","event_id":"$inv","sender":"@carol:x.test",
           "state_key":USER,"content":{"membership":"invite"}})
}

#[tokio::test]
async fn a_declined_invitation_in_a_world_readable_room_is_never_read() {
    for limited in [false, true] {
        let (server, seen) = declined(invite_state(), limited).await;
        assert!(seen.is_empty(), "limited {limited}: delivered {seen:?}");
        let asked = messages_for(&server, "a");
        assert_eq!(asked.len(), 1, "limited {limited}: {asked:?}");
        assert!(is_check(&asked[0]), "{asked:?}");
    }
}

#[tokio::test]
async fn a_join_sent_by_someone_else_is_no_acceptance() {
    let forged = json!({"type":"m.room.member","event_id":"$j","sender":"@carol:x.test",
                        "state_key":USER,"content":{"membership":"join"}});
    for limited in [false, true] {
        let (server, seen) = declined(forged.clone(), limited).await;
        assert!(seen.is_empty(), "limited {limited}: delivered {seen:?}");
        let asked = messages_for(&server, "a");
        assert!(asked.len() == 1 && is_check(&asked[0]), "{asked:?}");
    }
}

#[tokio::test]
async fn a_refused_check_is_named() {
    let server = invited("invite", refused).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider
        .fetch(one.next.as_ref())
        .await
        .expect("a refusal must not block");
    let cursor = cursor_json(&two);
    assert_eq!(cursor["given_up"][0]["room"], room_id("a"), "{cursor}");
    assert_eq!(cursor["given_up"][0]["why"], "check_refused", "{cursor}");
    assert_eq!(cursor.get("gaps"), None, "{cursor}");
    assert!(from_the_start(&messages_for(&server, "a")).is_empty());
}
