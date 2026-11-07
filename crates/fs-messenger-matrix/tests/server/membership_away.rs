//! A room the account left (away) and that comes back as an invitation: the return, a message and a
//! kick between two syncs, and the invitation again. Against the pinned tuwunel only.

use serde_json::json;

use crate::common::{Person, token};
use crate::http;
use crate::rejoin::{invite, together};

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn returned_spoken_to_kicked_and_invited_again_between_two_syncs_loses_nothing() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let (room, view, mine) = together(&alice, &carol).await;
    http::room_action(&mine, &room, "leave", json!({})).await;
    alice.sync().await; // the room is away now

    // None of this reaches Alice's provider until its next sync.
    invite(&view, &room, &alice).await;
    let joined = http::room_action(&mine, &room, "join", json!({})).await;
    assert!(joined.get("errcode").is_none(), "{joined}");
    let said = format!("WhileBack-{}", token());
    http::plain_message(&view, &room, &said).await;
    let kicked = http::room_action(&view, &room, "kick", json!({"user_id": alice.user_id})).await;
    assert!(kicked.get("errcode").is_none(), "{kicked}");
    invite(&view, &room, &alice).await;

    let got = alice.wait_for(&said).await;
    assert_eq!(got.body_text, said);
}
