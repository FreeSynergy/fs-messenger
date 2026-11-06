//! A plain room can be read, marked `none`, and never written to.

use fs_messenger_core::{Encryption, Error, Provider};
use fs_messenger_matrix::SCHEME_EVENT;
use serde_json::Value;

use crate::common::{Person, alias_of, encrypted_room, token};
use crate::http;

/// Carol, another client, opens a plain room with Alice and says something. Answers
/// `(room, carol's token, the text, the event)`.
async fn plain_setting(alice: &Person, carol: &Person) -> (String, String, String, String) {
    let view = carol.server_view().await;
    let room = http::plain_room(&view, &[&alice.user_id]).await;
    assert_eq!(alice.join_next_invite().await, room);
    let text = format!("PlainHello-{}", token());
    let event = http::plain_message(&view, &room, &text).await;
    (room, view, text, event)
}

fn messages_on_server(events: &[Value], sender: &str) -> usize {
    let from_sender = |e: &&Value| {
        e["sender"] == sender && (e["type"] == "m.room.message" || e["type"] == "m.room.encrypted")
    };
    events.iter().filter(from_sender).count()
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_plain_event_arrives_marked_none_with_its_original() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let (room, view, text, event) = plain_setting(&alice, &carol).await;

    let got = alice.wait_for(&text).await;
    assert_eq!(got.summary.encryption, Encryption::None);
    assert!(!got.summary.encryption.is_end_to_end());
    assert_eq!(got.body_text, text);
    assert_eq!(alias_of(&got, SCHEME_EVENT), event);
    // Byte for byte up to `unsigned.age`, which is different in every answer of the server.
    let original = String::from_utf8(got.original.unwrap().bytes).unwrap();
    let on_server = http::raw_event(&view, &room, &event).await;
    let until_age = |json: &str| json[..json.find("\"age\":").unwrap_or(json.len())].to_owned();
    assert_eq!(until_age(&original), until_age(&on_server));
    let parsed: Value = serde_json::from_str(&original).unwrap();
    assert_eq!(parsed["type"], "m.room.message");
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn sending_into_a_plain_room_is_refused_and_nothing_reaches_the_server() {
    let alice = Person::new("alice").await;
    let carol = Person::new("carol").await;
    let (room, view, text, _) = plain_setting(&alice, &carol).await;
    let got = alice.wait_for(&text).await;

    // The plan says so.
    let plan = alice
        .provider
        .encryption_plan(std::slice::from_ref(&room))
        .await
        .unwrap();
    assert_eq!(plan.encryption, Encryption::None);
    assert_eq!(plan.missing_keys, [room.clone()]);

    // By the room's id: the core's plan refuses.
    let needle = format!("MustNotLeave-{}", token());
    let refused = alice.send(&[&room], &needle).await.unwrap_err();
    assert!(
        matches!(
            refused,
            Error::EncryptionRefused {
                encryption: Encryption::None,
                ..
            }
        ),
        "{refused}"
    );

    // A reply: the core's plan sees only Carol, who has keys, so it passes. The provider sees the
    // room, and refuses by itself.
    let reply_plan = alice
        .provider
        .encryption_plan(std::slice::from_ref(&carol.user_id))
        .await
        .unwrap();
    assert_eq!(
        reply_plan.encryption,
        Encryption::Megolm,
        "the gap the provider has to close"
    );
    let refused = alice.reply(got.id(), &needle).await.unwrap_err();
    let Error::EncryptionRefused {
        encryption,
        missing_keys,
    } = refused
    else {
        panic!("{refused}");
    };
    assert_eq!(encryption, Encryption::None);
    assert_eq!(missing_keys, [room.clone()]);

    // Nothing of Alice's on the server, seen past the provider.
    let events = http::room_events(&view, &room).await;
    assert_eq!(messages_on_server(&events, &alice.user_id), 0, "{events:?}");
    assert!(!serde_json::to_string(&events).unwrap().contains(&needle));
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_plain_event_in_an_encrypted_room_is_still_none() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    // Bob's other client ignores the room's encryption. The mark goes by the event, not by the room.
    let view = bob.server_view().await;
    let text = format!("Unprotected-{}", token());
    http::plain_message(&view, &room, &text).await;
    let got = alice.wait_for(&text).await;
    assert_eq!(got.summary.encryption, Encryption::None);
}
