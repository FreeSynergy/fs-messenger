//! alice to bob, encrypted, through `Draft.send`, over every pair of submission and fetch.

use fs_messenger_core::{DeliveryState, Encryption, Messenger, SignatureCheck};
use fs_messenger_mail::mime::MailFields;

use crate::common::{
    Fetch, Person, Submit, Who, contains, message_id_of, raw_by_message_id, serial, token,
};

async fn round_trip(submit: Submit, fetch: Fetch) {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), fetch, submit).await;
    let bob = Person::open(Who::Bob, b.path(), fetch, submit).await;
    alice.knows(&bob).await;
    // Bob knows nothing of alice yet: he learns her key from the mail.

    let t = token();
    let text = format!("Subject-{t}\nBody-{t} Grüße\n\nlast line\n");
    let file: &[u8] = b"\x00binary\xffAttach-bytes\r\nand a line\n";
    let bob_addr = Who::Bob.address();
    let sent_id = alice
        .send(
            None,
            std::slice::from_ref(&bob_addr),
            &[],
            &text,
            &[("a.bin", "application/x-test", file)],
        )
        .await
        .expect("send");

    let got = bob.wait_for(&format!("Body-{t}")).await;
    assert_eq!(
        got.summary.encryption,
        Encryption::Openpgp,
        "{submit:?}/{fetch:?}"
    );
    assert_eq!(got.body_text, text);
    assert!(got.body_html.is_empty());
    assert_eq!(got.summary.envelope.name, format!("Subject-{t}"));
    assert_eq!(
        got.summary.envelope.attributed_to.as_deref(),
        Some(Who::Alice.address().as_str())
    );
    assert!(!got.outgoing);

    assert_eq!(got.attachments.len(), 1);
    let meta = &got.attachments[0];
    assert_eq!(meta.name, "a.bin");
    assert_eq!(meta.media_type, "application/x-test");
    assert_eq!(meta.size, file.len() as u64);
    assert_eq!(bob.repo.attachment(meta.blob).await.unwrap().bytes, file);

    // ADR 0030: what is stored is what the server has, byte for byte.
    let message_id = message_id_of(&got);
    let on_server = raw_by_message_id(Who::Bob, &message_id)
        .await
        .expect("on the server");
    assert_eq!(got.original.as_ref().unwrap().bytes, on_server);

    // First contact: the key came with the mail, so it is learned and the signature proves nothing yet.
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    let learned = bob
        .provider
        .peer(&Who::Alice.address())
        .await
        .unwrap()
        .expect("learned");
    let own = alice.provider.ensure_own_key().await.unwrap();
    assert_eq!(learned.fingerprint, own.fingerprint());

    // A reply: bob's answer lands in alice's thread, under her message.
    let reply = format!("Reply-{t}");
    bob.send(Some(got.id()), &[Who::Alice.address()], &[], &reply, &[])
        .await
        .expect("reply");
    let answer = alice.wait_for(&reply).await;
    assert_eq!(answer.summary.encryption, Encryption::Openpgp);
    assert_eq!(
        answer.summary.envelope.in_reply_to,
        Some(sent_id.to_string())
    );
    let own = alice.message(sent_id).await;
    assert_eq!(
        answer.summary.envelope.context,
        own.summary.envelope.context
    );

    // The sent copy came back with the sync and is not a second message.
    assert_eq!(
        alice.find(&format!("Body-{t}")).await.len(),
        1,
        "the sent copy is recognised"
    );
    assert_eq!(own.summary.state, DeliveryState::Sent);
    assert!(own.outgoing);
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn smtp_then_imap() {
    round_trip(Submit::Smtp, Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn smtp_then_jmap() {
    round_trip(Submit::Smtp, Fetch::Jmap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn jmap_then_imap() {
    round_trip(Submit::Jmap, Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn jmap_then_jmap() {
    round_trip(Submit::Jmap, Fetch::Jmap).await;
}

/// A hidden recipient can read the mail, and no header or copy on the server names him.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn bcc_can_read_and_is_named_nowhere() {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Imap, Submit::Smtp).await;
    alice.knows(&bob).await;
    let t = token();
    let text = format!("Hidden-{t}");
    alice
        .send(
            None,
            &[Who::Alice.address()],
            &[Who::Bob.address()],
            &text,
            &[],
        )
        .await
        .expect("send");
    let got = bob.wait_for(&text).await;
    assert_eq!(got.summary.encryption, Encryption::Openpgp);
    let id = message_id_of(&got);
    // The sent copy holds no trace of him. The delivered one has the server's own `Delivered-To`, the
    // one place that must name him, so check what the sender wrote: `To`, `Cc`, and the payload.
    let copy = raw_by_message_id(Who::Alice, &id)
        .await
        .expect("the sent copy");
    assert!(
        !contains(&copy, &Who::Bob.address()),
        "bcc in the sent copy"
    );
    let delivered = &got.original.as_ref().unwrap().bytes;
    let outer = MailFields::read(delivered).unwrap();
    assert_eq!(outer.to, [Who::Alice.address()]);
    assert!(outer.cc.is_empty());
    assert_eq!(got.summary.envelope.to, [Who::Alice.address()]);
}
