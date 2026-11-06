//! A batch the SDK saw and the core did not store comes again.

use fs_messenger_core::{Encryption, Provider};

use crate::common::{Person, encrypted_room, token};

/// The crash the core's cursor rule is for: the SDK saw a batch, the core never stored it. The SDK
/// drops a `/sync` answer it has seen, so without care the batch would be gone and the cursor past it.
#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_batch_the_core_never_stored_comes_again() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let before = bob.provider.fetch(None).await.unwrap().next.unwrap();
    let text = format!("Crashed-{}", token());
    alice.send(&[&room], &text).await.expect("send");

    let mut delivered = false;
    for _ in 0..40 {
        let batch = bob.provider.fetch(Some(&before)).await.unwrap();
        delivered = batch.messages.iter().any(|m| m.body_text == text);
        if delivered {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(
        delivered,
        "the first delivery, which is then lost in the crash"
    );

    // The same cursor again: the core restarted without having stored it.
    let again = bob.provider.fetch(Some(&before)).await.unwrap();
    let replayed = again.messages.iter().find(|m| m.body_text == text);
    let replayed = replayed.expect("the batch comes again");
    assert_eq!(replayed.encryption, Encryption::Megolm);

    // Through the core now, once.
    assert_eq!(bob.sync().await.stored, 1);
    assert_eq!(bob.find(&text).await.len(), 1);
    assert_eq!(bob.sync().await.stored, 0);
}
