//! Removed and invited again between two syncs: what the room said before is still read. Against the
//! pinned tuwunel only; Synapse was not asked.

use crate::common::{Person, token};
use crate::http;
use serde_json::json;

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn what_was_said_before_a_kick_and_a_new_invitation_arrives() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, room);
    let first = format!("First-{}", token());
    http::plain_message(&view, &room, &first).await;
    alice.wait_for(&first).await;

    // None of this reaches Alice's provider until its next sync, which then sees an invitation only.
    let said = format!("BeforeTheKick-{}", token());
    http::plain_message(&view, &room, &said).await;
    let kicked = http::room_action(&view, &room, "kick", json!({"user_id": alice.user_id})).await;
    assert!(kicked.get("errcode").is_none(), "{kicked}");
    let invited =
        http::room_action(&view, &room, "invite", json!({"user_id": alice.user_id})).await;
    assert!(invited.get("errcode").is_none(), "{invited}");

    let path = format!("/rooms/{}/messages?dir=b&limit=10", http::encode(&room));
    let (status, text) = http::get(&path, &alice.server_view().await).await;
    println!(
        "INVITED-AGAIN /messages status {status}, has it: {}",
        text.contains(&said)
    );
    let got = alice.wait_for(&said).await;
    assert_eq!(got.body_text, said);
}
