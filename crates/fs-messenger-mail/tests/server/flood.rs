//! What anyone can write without a key, on the real server.

use fs_messenger_mail::autocrypt::format_header;

use crate::common::{Fetch, Person, Submit, Who, serial, smtp_raw, token};

/// The third review ran this: public self-signatures copied with new unhashed padding, on forged plain
/// mail. No key is needed. Nothing is learned from it, and the key store does not grow.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_flood_of_padded_signatures_does_not_grow_the_key_store() {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Imap, Submit::Smtp).await;
    bob.knows(&alice).await;
    let from = Who::Alice.address();
    let genuine = alice.provider.ensure_own_key().await.unwrap().public();
    let before = bob.provider.peer(&from).await.unwrap().unwrap().cert;
    let t = token();
    for n in 0..8 {
        let form = crate::forge_subkeys::padded(&genuine, &format!("p{n}"), 40_000);
        let raw = format!(
            "From: {from}\r\nTo: {}\r\nSubject: f{n}\r\nMessage-ID: <{t}-{n}@msgtest.test>\r\nAutocrypt: {}\r\n\r\nFlood{n}-{t}\r\n",
            Who::Bob.address(),
            format_header(&from, &form).unwrap()
        );
        smtp_raw(Who::Bob, Who::Bob, raw.as_bytes()).await;
        bob.wait_for(&format!("Flood{n}-{t}")).await;
    }
    let peer = bob.provider.peer(&from).await.unwrap().unwrap();
    assert_eq!(peer.cert, before, "nothing new was learned");
    let log =
        fs_store::Store::activities(bob.key_store.as_ref(), &fs_store::ActivityFilter::default())
            .await
            .unwrap();
    assert!(
        format!("{log:?}").len() < 100_000,
        "the key log stays small"
    );
    drop(bob);
    let size: u64 = std::fs::metadata(b.path().join("keys.db")).unwrap().len();
    assert!(size < 1024 * 1024, "keys.db is {size} bytes");
}
