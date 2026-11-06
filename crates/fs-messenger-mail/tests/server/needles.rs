//! The proof of encrypted only: the bytes on the server, read separately over IMAP, hold none of it.

use crate::common::{Fetch, Person, Submit, Who, contains, raw_by_message_id, serial, token};

async fn no_needle(submit: Submit) {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, submit).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Imap, submit).await;
    alice.knows(&bob).await;
    let t = token();
    let subject = format!("SecretSubject{t}");
    let body = format!("SecretBody{t}");
    let file = format!("SecretFile{t}");
    let name = format!("secretname{t}.txt");
    let text = format!("{subject}\n{body}\n");
    alice
        .send(None, &[Who::Bob.address()], &[], &text, &[(&name, "text/plain", file.as_bytes())])
        .await
        .expect("send");
    let got = bob.wait_for(&body).await;
    assert_eq!(got.body_text, text, "the reader has it, so the needles are the right ones");
    let id = got.aliases.iter().find(|x| x.scheme == "message-id").unwrap().id.clone();

    let delivered = raw_by_message_id(Who::Bob, &id).await.expect("in bob's mailbox");
    let copy = raw_by_message_id(Who::Alice, &id).await.expect("in alice's sent mailbox");
    for (place, raw) in [("delivered", &delivered), ("sent copy", &copy)] {
        for needle in [&subject, &body, &file, &name, &t] {
            assert!(!contains(raw, needle), "{place} holds {needle}");
        }
        assert!(contains(raw, "multipart/encrypted"), "{place} is PGP/MIME");
        assert!(contains(raw, "-----BEGIN PGP MESSAGE-----"), "{place} is armored");
        assert!(contains(raw, "Subject: [...]"), "{place} has the placeholder subject");
        assert!(contains(raw, "Autocrypt: addr="), "{place} offers the sender's key");
    }
    // The server prepends `Delivered-To` and the like on delivery, so compare from the ciphertext on.
    let cipher = |raw: &[u8]| {
        let at = raw.windows(10).position(|w| w == b"-----BEGIN").expect("armor");
        let text = &raw[at..];
        let end = text.windows(8).position(|w| w == b"-----END").expect("armor end");
        text[..end].to_vec()
    };
    assert_eq!(cipher(&delivered), cipher(&copy), "the sent copy is the mail that left");
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn smtp_leaves_nothing_readable() {
    no_needle(Submit::Smtp).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn jmap_leaves_nothing_readable() {
    no_needle(Submit::Jmap).await;
}
