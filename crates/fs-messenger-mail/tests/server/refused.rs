//! ADR 0018: without a key for everyone, nothing leaves.

use std::time::Duration;

use fs_messenger_core::{Encryption, Error};

use crate::common::{Fetch, Person, Submit, Who, raw_mails, serial, token};

async fn refused(submit: Submit) {
    let _one = serial().await;
    let a = tempfile::tempdir().unwrap();
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, submit).await;
    let (bob_before, alice_before) = (
        raw_mails(Who::Bob).await.len(),
        raw_mails(Who::Alice).await.len(),
    );

    let keyless = Who::Bob.address();
    let text = format!("Never-{}", token());
    let Err(Error::EncryptionRefused {
        encryption,
        missing_keys,
    }) = alice.send(None, &[keyless.clone()], &[], &text, &[]).await
    else {
        panic!("must be refused");
    };
    assert_eq!(encryption, Encryption::None);
    assert_eq!(missing_keys, [keyless.clone()]);

    // Hidden recipients count, and only the one without a key is named.
    let stranger = "nobody@msgtest.test".to_string();
    let Err(Error::EncryptionRefused { missing_keys, .. }) = alice
        .send(
            None,
            &[Who::Alice.address()],
            &[stranger.clone()],
            &text,
            &[],
        )
        .await
    else {
        panic!("a hidden recipient without a key must refuse the mail");
    };
    assert_eq!(missing_keys, [stranger]);

    // Long enough for a delivery to show; a local one takes well under a second here.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        raw_mails(Who::Bob).await.len(),
        bob_before,
        "nothing reached bob"
    );
    assert_eq!(
        raw_mails(Who::Alice).await.len(),
        alice_before,
        "and no copy reached alice's"
    );
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn without_a_key_nothing_leaves_over_smtp() {
    refused(Submit::Smtp).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn without_a_key_nothing_leaves_over_jmap() {
    refused(Submit::Jmap).await;
}
