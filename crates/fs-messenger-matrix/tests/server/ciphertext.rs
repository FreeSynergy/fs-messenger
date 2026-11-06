//! What the server holds is ciphertext, and the original we keep is what the server holds.

use fs_messenger_core::Encryption;
use fs_messenger_matrix::SCHEME_EVENT;
use serde_json::Value;

use crate::common::{Person, alias_of, encrypted_room, token};
use crate::http;

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn the_server_never_sees_the_text() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let needle = format!("Needle-{}", token());
    alice.send(&[&room], &needle).await.expect("send");
    let at_bob = bob.wait_for(&needle).await;
    assert_eq!(at_bob.summary.encryption, Encryption::Megolm);
    let event = alias_of(&at_bob, SCHEME_EVENT);

    // A plain HTTP call, past the SDK, from a device that holds no keys.
    let view = bob.server_view().await;
    let on_server = http::raw_event(&view, &room, &event).await;
    let parsed: Value = serde_json::from_str(&on_server).unwrap();
    assert_eq!(parsed["type"], "m.room.encrypted");
    assert_eq!(parsed["content"]["algorithm"], "m.megolm.v1.aes-sha2");
    assert!(parsed["content"]["ciphertext"].is_string());
    assert!(
        !on_server.contains(&needle),
        "the needle is on the server: {on_server}"
    );
    let all = http::room_events(&view, &room).await;
    assert!(!serde_json::to_string(&all).unwrap().contains(&needle));

    // ADR 0030: the original is that event, byte for byte; the text stays out of it.
    // Only `unsigned.age` differs between two answers of the server, so the comparison stops there.
    let original = String::from_utf8(at_bob.original.expect("an original").bytes).unwrap();
    let until_age = |json: &str| json[..json.find("\"age\":").unwrap()].to_owned();
    assert_eq!(until_age(&original), until_age(&on_server));
    assert!(!original.contains(&needle));

    // Alice's own copy, after the provider recognised it.
    let mine = alice.find(&needle).await.pop().unwrap();
    assert_eq!(mine.summary.encryption, Encryption::Megolm);
    assert_eq!(alias_of(&mine, SCHEME_EVENT), event);
}
