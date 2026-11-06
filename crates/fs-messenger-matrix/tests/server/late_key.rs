//! A message without a key is stored visibly undecrypted; when the key comes, the text comes too.
//!
//! The key comes as a real one: from another device of the same user, as an encrypted export file.

use fs_messenger_core::{Encryption, Ingest};
use fs_messenger_matrix::{Password, SCHEME_EVENT, UNDECRYPTED_MARK};
use serde_json::Value;

use crate::common::{Person, alias_of, encrypted_room, token};

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
    let held = phone.find(UNDECRYPTED_MARK).await;
    let [held] = held.as_slice() else {
        panic!("one placeholder, not {}", held.len());
    };
    assert_eq!(
        held.summary.encryption,
        Encryption::Unknown,
        "never megolm without decryption"
    );
    assert!(!held.summary.encryption.is_end_to_end());
    assert!(held.undecrypted, "and the core is told, in its typed field");
    assert!(phone.find(&text).await.is_empty(), "the text is not here");
    let original: Value = serde_json::from_slice(&held.original.as_ref().unwrap().bytes).unwrap();
    assert_eq!(original["type"], "m.room.encrypted", "the original is kept");
    let event = alias_of(held, SCHEME_EVENT);
    assert_eq!(original["event_id"], event.as_str());

    // Nothing changes on its own.
    let quiet = phone.sync().await;
    assert_eq!(quiet.stored, 0);

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

    let late = phone.wait_for(&text).await;
    assert_eq!(
        late.summary.encryption,
        Encryption::Megolm,
        "decryption really happened now"
    );
    assert_eq!(late.body_text, text);
    assert!(!late.undecrypted);
    assert_eq!(
        late.summary.envelope.in_reply_to,
        Some(held.id().to_string()),
        "it answers the placeholder"
    );
    assert_eq!(late.original, held.original, "same event, same original");

    // What the core allows: not replaced. The placeholder stays beside the text. See DESIGN.md.
    assert_eq!(phone.find(UNDECRYPTED_MARK).await.len(), 1);

    // Replays add nothing, and the waiting list came back empty with the cursor.
    let once_more = phone.sync().await;
    assert_eq!(once_more.stored, 0);
    let phone = phone.restart().await;
    assert_eq!(phone.sync().await.stored, 0);
    let replayed = phone.repo.sync(phone.account.id).await.unwrap();
    assert_eq!(replayed.stored, 0);
    assert_eq!(phone.find(&text).await.len(), 1);
}
