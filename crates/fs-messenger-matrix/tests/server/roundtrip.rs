//! Alice to Bob through the core's `Draft.send`, and back: text, threading, `megolm`.

use fs_messenger_core::{Encryption, Messenger};
use fs_messenger_matrix::SCHEME_EVENT;

use crate::common::{Person, alias_of, encrypted_room, token};

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn alice_to_bob_and_back_with_threading() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;

    let text = format!("Hello-{}", token());
    let plan_passes = alice.send(&[&room], &text).await.expect("send");
    let at_bob = bob.wait_for(&text).await;
    assert_eq!(at_bob.body_text, text);
    assert_eq!(at_bob.summary.encryption, Encryption::Megolm);
    assert_eq!(
        at_bob.summary.envelope.attributed_to.as_deref(),
        Some(alice.user_id.as_str())
    );
    assert!(!at_bob.outgoing);
    let event = alias_of(&at_bob, SCHEME_EVENT);
    assert!(event.starts_with('$'), "the event id is the alias: {event}");

    // Bob answers: the reply knows its parent on both sides, in the same conversation.
    let answer = format!("Answer-{}", token());
    let answered = bob.reply(at_bob.id(), &answer).await.expect("reply");
    let at_alice = alice.wait_for(&answer).await;
    assert_eq!(at_alice.summary.encryption, Encryption::Megolm);
    assert_eq!(
        at_alice.summary.envelope.in_reply_to,
        Some(plan_passes.to_string()),
        "threading from m.in_reply_to, resolved to alice's own message"
    );
    let at_bob_own = bob.find(&answer).await.pop().unwrap();
    assert_eq!(at_bob_own.id(), answered);
    assert_eq!(
        at_bob_own.summary.envelope.in_reply_to,
        Some(at_bob.id().to_string())
    );
    assert_eq!(
        at_bob_own.summary.envelope.context,
        at_bob.summary.envelope.context
    );

    // The room is the conversation on both sides.
    let conversation = at_alice
        .summary
        .envelope
        .context
        .clone()
        .unwrap()
        .parse()
        .unwrap();
    let opened = alice.repo.open(conversation).await.unwrap();
    assert!(!opened.name.is_empty());
    let mine = alice.find(&text).await.pop().unwrap();
    assert_eq!(
        mine.summary.envelope.context.as_deref(),
        Some(conversation.to_string().as_str())
    );
    assert!(mine.outgoing);
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn writing_to_a_person_opens_an_encrypted_room() {
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;

    let text = format!("First-{}", token());
    alice.send(&[&bob.user_id], &text).await.expect("send");
    let room = bob.join_next_invite().await;
    let got = bob.wait_for(&text).await;
    assert_eq!(got.summary.encryption, Encryption::Megolm);

    // The second message finds the same room.
    let second = format!("Second-{}", token());
    alice.sync().await;
    alice.send(&[&bob.user_id], &second).await.expect("send");
    let got = bob.wait_for(&second).await;
    assert_eq!(got.summary.encryption, Encryption::Megolm);
    let token_a = alice.server_view().await;
    let events = crate::http::room_events(&token_a, &room).await;
    assert!(events.iter().any(|e| e["type"] == "m.room.encrypted"));
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn someone_without_keys_is_named_and_nothing_leaves() {
    let alice = Person::new("alice").await;
    // Registered, and never logged in with a client: no device has keys.
    let (local, password) = crate::common::new_user("carol");
    crate::http::register(&local, &password).await;
    let carol = crate::common::mxid(&local);

    let plan = {
        use fs_messenger_core::Provider;
        alice
            .provider
            .encryption_plan(std::slice::from_ref(&carol))
            .await
            .unwrap()
    };
    assert_eq!(plan.missing_keys, std::slice::from_ref(&carol));

    let text = format!("NotSent-{}", token());
    let refused = alice.send(&[&carol], &text).await.unwrap_err();
    let fs_messenger_core::Error::EncryptionRefused { missing_keys, .. } = refused else {
        panic!("{refused}");
    };
    assert_eq!(missing_keys, [carol]);
    assert!(
        alice.find(&text).await.is_empty(),
        "no message, only the saved draft"
    );
}
