//! A restart resumes from the cursor, a replay duplicates nothing, keys and identity survive.

use fs_messenger_core::{Encryption, Messenger, Provider};
use fs_messenger_matrix::SCHEME_EVENT;

use crate::common::{Person, alias_of, encrypted_room, token};
use crate::http;

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_restart_resumes_and_nothing_is_stored_twice() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let before = format!("BeforeRestart-{}", token());
    alice.send(&[&room], &before).await.expect("send");
    bob.wait_for(&before).await;
    let quiet = bob.sync().await;
    assert_eq!(
        (quiet.stored, quiet.duplicates),
        (0, 0),
        "nothing new, nothing replayed"
    );

    // Own echo, in one process: stored once, by the receipt or by the provider's own list.
    alice.sync().await;
    alice.sync().await;
    assert_eq!(
        alice.find(&before).await.len(),
        1,
        "own echo is not stored twice"
    );

    let bob = bob.restart().await;
    let after_restart = bob.sync().await;
    assert_eq!(
        after_restart.stored, 0,
        "the cursor came back with the account"
    );
    assert_eq!(bob.find(&before).await.len(), 1);

    // A new message reaches the restarted Bob, and only that one is new.
    let after = format!("AfterRestart-{}", token());
    alice.send(&[&room], &after).await.expect("send");
    bob.wait_for(&after).await;
    assert_eq!(bob.find(&before).await.len(), 1);
    assert_eq!(bob.find(&after).await.len(), 1);

    // A replay from the very start, through the real provider: the same event ids, so the core
    // stores nothing. This is the fetch of a core that lost its cursor.
    let replayed = bob.provider.fetch(None).await.unwrap();
    assert!(!replayed.messages.is_empty(), "the server did replay");
    for raw in replayed.messages {
        let result = fs_messenger_core::Ingest::ingest(&bob.repo, bob.account.id, raw).await;
        assert!(
            matches!(result, Ok(fs_messenger_core::Ingested::Duplicate(_))),
            "{result:?}"
        );
    }
    assert_eq!(bob.find(&before).await.len(), 1);
    assert_eq!(bob.find(&after).await.len(), 1);

    // Alice restarted: her provider's list is gone, so now the core's receipt does the work.
    let alice = alice.restart().await;
    let replayed = alice.provider.fetch(None).await.unwrap();
    let own = replayed.messages.iter().filter(|m| m.outgoing).count();
    assert!(own >= 2, "her own messages came back");
    for raw in replayed.messages {
        let result = fs_messenger_core::Ingest::ingest(&alice.repo, alice.account.id, raw).await;
        result.expect("ingest");
    }
    assert_eq!(
        alice.find(&before).await.len(),
        1,
        "her copy met its receipt"
    );
    assert_eq!(alice.find(&after).await.len(), 1);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn session_and_keys_survive_a_restart() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let first = format!("KeyArrives-{}", token());
    alice.send(&[&room], &first).await.expect("send");
    let got = bob.wait_for(&first).await;
    assert_eq!(got.summary.encryption, Encryption::Megolm);

    let view = bob.server_view().await;
    let device = bob.provider.device_id().unwrap();
    let identity = bob.provider.identity_key().await.unwrap();
    let devices = http::devices(&view).await.len();

    // Written while Bob is gone, with the session he already holds: no new key will come. Only the
    // key on his disk can open it.
    let bob = bob.restart().await;
    let while_away = format!("WhileAway-{}", token());
    alice.send(&[&room], &while_away).await.expect("send");
    let got = bob.wait_for(&while_away).await;
    assert_eq!(got.summary.encryption, Encryption::Megolm);
    assert_eq!(got.body_text, while_away);

    assert_eq!(bob.provider.device_id().unwrap(), device, "same device");
    assert_eq!(
        bob.provider.identity_key().await.unwrap(),
        identity,
        "same identity"
    );
    assert_eq!(
        http::devices(&view).await.len(),
        devices,
        "no new device on the server"
    );

    // Bob's own sending key survives too, and Alice still reads him.
    let answer = format!("BobAfterRestart-{}", token());
    bob.send(&[&room], &answer).await.expect("send");
    let at_alice = alice.wait_for(&answer).await;
    assert_eq!(at_alice.summary.encryption, Encryption::Megolm);

    // A second restart in a row: what the first one rebuilt was written back properly.
    let bob = bob.restart().await;
    let replayed = bob.provider.fetch(None).await.unwrap();
    let opened = replayed
        .messages
        .iter()
        .filter(|m| m.encryption == Encryption::Megolm);
    assert!(
        opened.count() >= 3,
        "old messages still decrypt after two restarts"
    );
    let event = alias_of(&bob.find(&first).await.pop().unwrap(), SCHEME_EVENT);
    assert!(event.starts_with('$'));
    assert_eq!(bob.repo.accounts().await.unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn a_gap_in_the_timeline_is_closed() {
    let alice = Person::new("alice").await;
    let bob = Person::with_limit("bob", Some(3)).await;
    let room = encrypted_room(&alice, &bob).await;

    let batch = token();
    let mut texts = Vec::new();
    for i in 0..12 {
        let text = format!("Gap-{batch}-{i:02}");
        alice.send(&[&room], &text).await.expect("send");
        texts.push(text);
    }
    bob.wait_for(texts.last().unwrap()).await;
    for text in &texts {
        let got = bob.find(text).await;
        assert_eq!(
            got.len(),
            1,
            "{text} arrived once, though /sync gave only 3 per room"
        );
        assert_eq!(got[0].summary.encryption, Encryption::Megolm);
    }
}

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
