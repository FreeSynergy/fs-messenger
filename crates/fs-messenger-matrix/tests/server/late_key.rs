//! A message without a key is stored visibly undecrypted, and replaced in place when the key comes.
//!
//! The key comes as a real one: from another device of the same user, as an encrypted export file.

use fs_messenger_core::Encryption;
use fs_messenger_matrix::{Password, UNDECRYPTED_MARK};
use serde_json::Value;

use crate::common::{Person, encrypted_room, event_id_of, token};

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn undecrypted_then_the_key_arrives() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let text = format!("LateKey-{}", token());
    alice.send(&[&room], &text).await.expect("send");
    bob.wait_for(&text).await;

    // A device that logged in after the message: nobody shared the key with it.
    let phone = bob.second_device().await;
    let held = phone.undecrypted().await;
    let [held] = held.as_slice() else {
        panic!("one placeholder, not {}", held.len());
    };
    assert_eq!(held.body_text, UNDECRYPTED_MARK);
    assert_eq!(
        held.summary.encryption,
        Encryption::Unknown,
        "never megolm without decryption"
    );
    assert!(!held.summary.encryption.is_end_to_end());
    assert!(phone.find(&text).await.is_empty(), "the text is not here");
    let original: Value = serde_json::from_slice(&held.original.as_ref().unwrap().bytes).unwrap();
    assert_eq!(original["type"], "m.room.encrypted", "the original is kept");
    let event = event_id_of(held);
    assert_eq!(original["event_id"], event.as_str());

    // Nothing changes on its own.
    assert_eq!(phone.sync().await.stored, 0);
    assert_eq!(phone.undecrypted().await.len(), 1);

    // The key arrives.
    let file = phone.file("keys.txt");
    let passphrase = Password::new("throwaway-export-passphrase");
    bob.provider
        .export_room_keys(file.clone(), &passphrase)
        .await
        .unwrap();
    assert!(
        phone
            .provider
            .import_room_keys(file, &passphrase)
            .await
            .unwrap()
            >= 1
    );

    // Replaced in place: the same message, the same original, now readable.
    let late = phone.wait_for(&text).await;
    assert_eq!(late.id(), held.id(), "the same message, not a second one");
    assert_eq!(
        late.summary.encryption,
        Encryption::Megolm,
        "decryption really happened now"
    );
    assert_eq!(late.body_text, text);
    assert!(!late.summary.undecrypted);
    assert_eq!(
        late.original, held.original,
        "ADR 0030: the original never changes"
    );
    assert!(
        phone.undecrypted().await.is_empty(),
        "no placeholder is left"
    );

    // Replays add nothing, and the waiting list came back empty with the cursor.
    assert_eq!(phone.sync().await.stored, 0);
    let phone = phone.restart().await;
    assert_eq!(phone.sync().await.stored, 0);
    assert_eq!(phone.find(&text).await.len(), 1);
    assert_eq!(phone.find(&text).await[0].id(), held.id());
}
