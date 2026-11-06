//! A key that arrives later: the same mail, read again, replaces its undecrypted form in the real core.

mod common;
#[path = "common/forge.rs"]
mod forge;

use common::{core_with, person};
use fs_messenger_core::{Encryption, Ingest, Ingested, Messenger, Provider};
use fs_messenger_mail::openpgp::generate_key;

#[tokio::test]
async fn a_replay_with_the_key_now_held_makes_the_mail_readable() {
    let address = "b@example.org";
    let bob = person(address).await;
    let (repo, account) = core_with(&bob).await;
    let (signer, older) = (
        generate_key("a@example.org").unwrap(),
        generate_key(address).unwrap(),
    );
    let head = "From: a@example.org\r\nMessage-ID: <late@e>\r\n";
    let inner =
        "From: a@example.org\r\nSubject: real\r\nContent-Type: text/plain\r\n\r\nreadable now\r\n";
    let raw = forge::encrypted(head, inner.as_bytes(), &signer, &older.public());

    bob.wire.inbox.lock().unwrap().push(raw.clone());
    let first = bob
        .provider
        .fetch(None)
        .await
        .unwrap()
        .messages
        .pop()
        .unwrap();
    assert!(first.undecrypted);
    let Ingested::Stored(id) = repo.ingest(account, first).await.unwrap() else {
        panic!("new");
    };

    // The older key comes back, and the mail is read again under the same id of the provider.
    bob.keys.put_own_key(address, &older).await.unwrap();
    bob.wire.inbox.lock().unwrap().push(raw);
    let second = bob
        .provider
        .fetch(None)
        .await
        .unwrap()
        .messages
        .pop()
        .unwrap();
    assert!(!second.undecrypted);
    assert!(matches!(
        repo.ingest(account, second).await.unwrap(),
        Ingested::Duplicate(_)
    ));

    let conversation = repo
        .search(
            "readable now",
            &fs_messenger_core::SearchFilter::default(),
            None,
        )
        .await
        .unwrap();
    let summary = &conversation.items[0];
    assert_eq!(summary.envelope.id, id);
    assert_eq!(summary.encryption, Encryption::Openpgp);
    assert!(!summary.undecrypted);
}
