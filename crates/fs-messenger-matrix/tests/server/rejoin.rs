//! Leaving a room and coming back: what was said while away is read again, as in a new room.

use crate::common::{Person, token};
use crate::http;
use serde_json::json;

/// Alice and Carol in a plain room, one message read. Answers `(room, carol's token, alice's token)`.
async fn together(alice: &Person, carol: &Person) -> (String, String, String) {
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, room);
    let first = format!("Before-{}", token());
    http::plain_message(&view, &room, &first).await;
    alice.wait_for(&first).await;
    (room, view, alice.server_view().await)
}

async fn invite(carol_view: &str, room: &str, who: &Person) {
    let answer =
        http::room_action(carol_view, room, "invite", json!({"user_id": who.user_id})).await;
    assert!(answer.get("errcode").is_none(), "{answer}");
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn what_was_said_while_away_is_read_after_the_return() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let (room, view, alice_view) = together(&alice, &carol).await;

    http::room_action(&alice_view, &room, "leave", json!({})).await;
    let missed = format!("Missed-{}", token());
    http::plain_message(&view, &room, &missed).await;
    alice.sync().await; // sees the leave; the message is not hers to receive
    assert!(alice.find(&missed).await.is_empty());

    invite(&view, &room, &alice).await;
    assert_eq!(alice.join_next_invite().await, room);
    let got = alice.wait_for(&missed).await;
    assert_eq!(got.body_text, missed);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn leaving_and_returning_between_two_syncs_loses_nothing() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let (room, view, alice_view) = together(&alice, &carol).await;

    // Alice's provider sees none of this until its next sync: leave, a message, invitation, join.
    http::room_action(&alice_view, &room, "leave", json!({})).await;
    let missed = format!("Between-{}", token());
    http::plain_message(&view, &room, &missed).await;
    invite(&view, &room, &alice).await;
    let joined = http::room_action(&alice_view, &room, "join", json!({})).await;
    assert!(joined.get("errcode").is_none(), "{joined}");

    let got = alice.wait_for(&missed).await;
    assert_eq!(got.body_text, missed);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_refused_invitation_does_not_hold_the_account_up() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    for _ in 0..80 {
        alice.sync().await;
        if !alice.provider.invites().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let alice_view = alice.server_view().await;
    http::room_action(&alice_view, &room, "leave", json!({})).await;
    http::plain_message(&view, &room, "Not for Alice").await;
    alice.sync().await;
    alice.sync().await;

    // Invited again later, she joins and reads what the room said.
    invite(&view, &room, &alice).await;
    assert_eq!(alice.join_next_invite().await, room);
    let later = format!("After-{}", token());
    http::plain_message(&view, &room, &later).await;
    alice.wait_for(&later).await;
}
