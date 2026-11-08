//! A room the cursor never saw as accepted, against the real server: a declined invitation, a leave
//! and a new invitation, a cursor the core did not keep, a knock. Against the pinned tuwunel only.

use serde_json::json;

use crate::common::{Person, token};
use crate::{http, rooms};

/// Carol makes a plain room and invites Alice. Answers `(room, carol's token)`.
async fn invite(alice: &Person, carol: &Person) -> (String, String) {
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    (room, view)
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_declined_world_readable_invitation_is_not_read() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    alice.sync().await;
    let view = carol.server_view().await;
    let open = json!({"history_visibility": "world_readable"});
    let state = [("m.room.history_visibility", open)];
    let room = rooms::room_with(&view, &[&alice.user_id], &state).await;
    let spam = format!("Spam-{}", token());
    http::plain_message(&view, &room, &spam).await;
    http::room_action(&alice.server_view().await, &room, "leave", json!({})).await;
    http::plain_message(&view, &room, &format!("Later-{}", token())).await;
    for _ in 0..3 {
        alice.sync().await;
    }
    assert!(
        alice.find(&spam).await.is_empty(),
        "content of a room never accepted was stored"
    );
    assert_eq!(
        alice.find("Later-").await,
        [] as [fs_messenger_core::Message; 0]
    );
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn unknown_room_left_and_invited_again() {
    // Joined, spoken in, left, invited again: all between two syncs, in a room the cursor never saw.
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    alice.sync().await;
    let mine = alice.server_view().await;
    let (room, view) = invite(&alice, &carol).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    let said = format!("WhileJoined-{}", token());
    http::plain_message(&view, &room, &said).await;
    http::room_action(&mine, &room, "leave", json!({})).await;
    http::room_action(&view, &room, "invite", json!({"user_id": alice.user_id})).await;
    let got = alice.wait_for(&said).await;
    assert_eq!(got.body_text, said);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_cursor_not_kept_replays_join_and_leave() {
    use fs_messenger_core::Provider;
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let before = alice.provider.fetch(None).await.unwrap().next.unwrap();
    let mine = alice.server_view().await;
    let (room, view) = invite(&alice, &carol).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    let said = format!("Replayed-{}", token());
    http::plain_message(&view, &room, &said).await;
    http::room_action(&mine, &room, "leave", json!({})).await;
    let mut seen = Vec::new();
    for _ in 0..2 {
        // The cursor the core "did not keep": the same old one each time.
        let batch = alice.provider.fetch(Some(&before)).await.unwrap();
        seen.push(batch.messages.iter().any(|m| m.body_text == said));
    }
    assert_eq!(seen, [true, true], "the second fetch is the replay");
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn knock() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    alice.sync().await;
    let view = carol.server_view().await;
    let rule = json!({"join_rule": "knock"});
    let open = json!({"history_visibility": "world_readable"});
    let state = [
        ("m.room.join_rules", rule),
        ("m.room.history_visibility", open),
    ];
    let room = rooms::room_with(&view, &[], &state).await;
    let secret = format!("Knock-{}", token());
    http::plain_message(&view, &room, &secret).await;
    let knocked = rooms::knock(&alice.server_view().await, &room).await;
    println!("T7 knock answer: {knocked}");
    for _ in 0..3 {
        alice.sync().await;
    }
    assert!(
        alice.find(&secret).await.is_empty(),
        "a knock is no acceptance"
    );
}

/// The own member events of `user` in the answer of `/sync` for `room`, in its `leave` part.
fn own_events(sync: &serde_json::Value, room: &str, user: &str) -> Vec<serde_json::Value> {
    let part = &sync["rooms"]["leave"][room];
    let state = part["state"]["events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let line = part["timeline"]["events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let member = |e: &&serde_json::Value| e["type"] == "m.room.member" && e["state_key"] == user;
    state
        .iter()
        .chain(line.iter())
        .filter(member)
        .cloned()
        .collect()
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_declined_invitation_leaves_no_join_in_unsigned() {
    // F4 of the ninth review: the evidence counts an own `leave` whose `unsigned.prev_content` is
    // `join`. What the real server shows for a room the account only declined (local invitation;
    // federation is off in the test server, so a foreign `make_leave` template is not tried).
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    alice.sync().await;
    let mine = alice.server_view().await;
    let (room, _view) = invite(&alice, &carol).await;
    let (_, first) = http::get("/sync?timeout=0", &mine).await;
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    let since = first["next_batch"].as_str().unwrap().to_owned();
    http::room_action(&mine, &room, "leave", json!({})).await;
    let (_, next) = http::get(&format!("/sync?timeout=0&since={since}"), &mine).await;
    let next: serde_json::Value = serde_json::from_str(&next).unwrap();
    let own = own_events(&next, &room, &alice.user_id);
    println!("F4 declined invitation, own member events in `leave`: {own:?}");
    assert!(
        !own.is_empty(),
        "the decline should show under leave: {next}"
    );
    for event in &own {
        let before = &event["unsigned"]["prev_content"]["membership"];
        assert_ne!(
            before, "join",
            "a declined invitation shows a join: {event}"
        );
    }
}
