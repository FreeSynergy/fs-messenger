//! A ban: what the real server answers to `/messages` for the banned account, and that the provider
//! goes on with its other rooms. Against the pinned tuwunel only; Synapse was not asked.

use crate::common::{Person, token};
use crate::http;
use serde_json::json;

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_banned_account_goes_on_with_its_other_rooms() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, room);
    let before = format!("BeforeBan-{}", token());
    http::plain_message(&view, &room, &before).await;
    alice.wait_for(&before).await;

    let banned = http::room_action(&view, &room, "ban", json!({"user_id": alice.user_id})).await;
    assert!(banned.get("errcode").is_none(), "{banned}");
    let during = format!("DuringBan-{}", token());
    http::plain_message(&view, &room, &during).await;
    alice.sync().await;

    // What the server says to the banned account's own read. Recorded in DESIGN.md.
    let path = format!("/rooms/{}/messages?dir=b&limit=10", http::encode(&room));
    let (status, text) = http::get(&path, &alice.server_view().await).await;
    println!("BAN /messages status {status}, {} bytes", text.len());
    assert!(
        !text.contains(&during),
        "a banned account read what came after the ban"
    );
    assert!(
        alice.find(&during).await.is_empty(),
        "a banned account got a message"
    );

    // Another room, after the ban, and a second sync: the account is not blocked.
    let other = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, other);
    let after = format!("OtherRoom-{}", token());
    http::plain_message(&view, &other, &after).await;
    alice.wait_for(&after).await;
    alice.sync().await;
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn what_a_forgotten_room_answers() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, room);
    let mine = alice.server_view().await;
    http::room_action(&mine, &room, "leave", json!({})).await;
    http::room_action(&mine, &room, "forget", json!({})).await;
    let path = format!("/rooms/{}/messages?dir=b&limit=10", http::encode(&room));
    let (status, text) = http::get(&path, &mine).await;
    println!(
        "FORGET /messages status {status}: {}",
        &text[..text.len().min(120)]
    );
    alice.sync().await;
    alice.sync().await;
}
