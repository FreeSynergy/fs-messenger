//! What the real server shows of rooms joined and left (or never joined) between two syncs. These
//! answer the assumptions of the design (`DESIGN.md`, "Not verified") and print what they saw.
//! Against the pinned tuwunel only; Synapse was not asked.

use serde_json::{Value, json};

use crate::common::{Person, token};
use crate::http;

/// Carol makes a plain room and invites Alice. Answers `(room, carol's token)`.
async fn invite(alice: &Person, carol: &Person) -> (String, String) {
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    (room, view)
}

fn events_of<'a>(answer: &'a Value, section: &str, room: &str) -> Vec<&'a Value> {
    let r = &answer["rooms"][section][room];
    let timeline = r["timeline"]["events"].as_array().into_iter().flatten();
    let state = r["state"]["events"].as_array().into_iter().flatten();
    timeline.chain(state).collect()
}

fn own_join_in(events: &[&Value], own: &str) -> bool {
    events.iter().any(|e| {
        e["type"] == "m.room.member"
            && e["sender"] == own
            && e["state_key"] == own
            && e["content"]["membership"] == "join"
    })
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn joined_and_left_between_two_syncs_shows_the_own_join() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let mine = alice.server_view().await;
    let (_, first) = (0, http::sync_raw(&mine, None).await);
    let since = first.1["next_batch"].as_str().unwrap().to_owned();
    let (room, view) = invite(&alice, &carol).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    let said = format!("InAndOut-{}", token());
    http::plain_message(&view, &room, &said).await;
    http::room_action(&mine, &room, "leave", json!({})).await;

    let (status, answer) = http::sync_raw(&mine, Some(&since)).await;
    let leave = events_of(&answer, "leave", &room);
    let joined = own_join_in(&leave, &alice.user_id);
    println!(
        "T1 status {status}; room under leave: {}; own join visible there: {joined}; events: {}",
        answer["rooms"]["leave"].get(&room).is_some(),
        leave.len()
    );
    assert_eq!(status, 200);
    assert!(joined, "the own join is not in the leave answer: {answer}");

    let got = alice.wait_for(&said).await;
    assert_eq!(got.body_text, said);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn the_check_filter_is_honoured() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let mine = alice.server_view().await;
    let (room, view) = invite(&alice, &carol).await;
    http::plain_message(&view, &room, &format!("Hidden-{}", token())).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    http::plain_message(&view, &room, &format!("Hidden2-{}", token())).await;
    http::room_action(&mine, &room, "leave", json!({})).await;
    http::room_action(&view, &room, "invite", json!({"user_id": alice.user_id})).await;

    // Asked as the invited-again account, which is the point: what does the check find then?
    let (status, body) = http::own_member_events(&mine, &room, &alice.user_id).await;
    let chunk = body["chunk"].as_array().cloned().unwrap_or_default();
    let only_own = chunk
        .iter()
        .all(|e| e["type"] == "m.room.member" && e["sender"] == alice.user_id.as_str());
    let kinds: Vec<_> = chunk
        .iter()
        .map(|e| {
            (
                e["content"]["membership"].clone(),
                e["unsigned"]["prev_content"]["membership"].clone(),
            )
        })
        .collect();
    println!(
        "T4 status {status}; {} events; only own member events: {only_own}; (membership, previous): {kinds:?}",
        chunk.len()
    );
    assert_eq!(status, 200, "{body}");
    assert!(only_own, "the filter was not honoured: {body}");
    // Recorded: an invited-again account is shown its own LEAVE (previous membership `join`), not
    // its earlier join. So an own leave after a join is evidence too.
    let proof = chunk.iter().any(|e| {
        e["content"]["membership"] == "leave"
            && e["unsigned"]["prev_content"]["membership"] == "join"
    });
    assert!(proof, "no evidence of the earlier join: {body}");
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_kick_is_not_shown_by_the_own_sender_filter() {
    // Recorded as a limit, not a wish: the filter keeps own events only, and a kick is sent by the
    // other person. A room kicked from and invited again between two syncs, never read before, has
    // no evidence the check can find (DESIGN.md, "Open points").
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let mine = alice.server_view().await;
    let (room, view) = invite(&alice, &carol).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    http::plain_message(&view, &room, &format!("Kicked-{}", token())).await;
    http::room_action(&view, &room, "kick", json!({"user_id": alice.user_id})).await;
    http::room_action(&view, &room, "invite", json!({"user_id": alice.user_id})).await;
    let (status, body) = http::own_member_events(&mine, &room, &alice.user_id).await;
    let found = body["chunk"].as_array().map_or(0, Vec::len);
    println!(
        "T4b status {status}; own member events visible after a kick and a new invitation: {found}"
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(found, 0, "{body}");
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
    let room = http::room_with(&view, &[&alice.user_id], &state).await;
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
    assert!(alice.find("Later-").await.is_empty());
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
    let room = http::room_with(&view, &[], &state).await;
    let secret = format!("Knock-{}", token());
    http::plain_message(&view, &room, &secret).await;
    let knocked = http::knock(&alice.server_view().await, &room).await;
    println!("T7 knock answer: {knocked}");
    for _ in 0..3 {
        alice.sync().await;
    }
    assert!(
        alice.find(&secret).await.is_empty(),
        "a knock is no acceptance"
    );
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_sync_from_an_old_token_answers() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let mine = alice.server_view().await;
    let (_, first) = (0, http::sync_raw(&mine, None).await);
    let t1 = first.1["next_batch"].as_str().unwrap().to_owned();
    let (room, view) = invite(&alice, &carol).await;
    http::room_action(&mine, &room, "join", json!({})).await;
    http::plain_message(&view, &room, &format!("Old-{}", token())).await;
    let (_, second) = http::sync_raw(&mine, Some(&t1)).await;
    let t2 = second["next_batch"].as_str().unwrap().to_owned();
    http::room_action(&mine, &room, "leave", json!({})).await;
    let (_, third) = http::sync_raw(&mine, Some(&t2)).await;
    let t3 = third["next_batch"].as_str().unwrap().to_owned();
    let (_, fourth) = http::sync_raw(&mine, Some(&t3)).await;
    assert!(fourth["next_batch"].is_string());

    let (status, old) = http::sync_raw(&mine, Some(&t1)).await;
    let leave = events_of(&old, "leave", &room);
    println!(
        "T5 status {status}; room under join: {}, leave: {}; own join visible: {}",
        old["rooms"]["join"].get(&room).is_some(),
        old["rooms"]["leave"].get(&room).is_some(),
        own_join_in(&leave, &alice.user_id)
    );
    assert_eq!(status, 200, "{old}");
    assert!(own_join_in(&leave, &alice.user_id), "{old}");
}
