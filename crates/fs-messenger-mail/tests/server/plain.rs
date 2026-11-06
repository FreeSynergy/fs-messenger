//! A plain mail arrives marked `none`, and a mail we cannot open stays visibly undecrypted.

use fs_messenger_core::Encryption;
use fs_messenger_mail::UNDECRYPTED_MARK;

use crate::common::{Fetch, Person, Submit, Who, contains, raw_by_message_id, serial, smtp_raw, token};

async fn plain(fetch: Fetch) {
    let _one = serial().await;
    let b = tempfile::tempdir().unwrap();
    let bob = Person::open(Who::Bob, b.path(), fetch, Submit::Smtp).await;
    let t = token();
    let id = format!("{t}@msgtest.test");
    let raw = format!(
        "From: {}\r\nTo: {}\r\nSubject: Plain{t}\r\nMessage-ID: <{id}>\r\n\
         Date: Fri, 06 Nov 2026 12:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlainBody{t}\r\n",
        Who::Alice.address(),
        Who::Bob.address()
    );
    smtp_raw(Who::Alice, Who::Bob, raw.as_bytes()).await;
    let got = bob.wait_for(&format!("PlainBody{t}")).await;
    assert_eq!(got.summary.encryption, Encryption::None, "never more than it is");
    // `trim_end`: lettre ends the DATA with one more CRLF of its own.
    assert_eq!(got.body_text.trim_end(), format!("PlainBody{t}"));
    assert_eq!(got.summary.envelope.name, format!("Plain{t}"));
    // The control of the needle check: on a plain mail the same check does find the text.
    let on_server = raw_by_message_id(Who::Bob, &id).await.unwrap();
    assert!(contains(&on_server, &format!("PlainBody{t}")));
    assert!(contains(&got.original.as_ref().unwrap().bytes, &format!("PlainBody{t}")));
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn plain_over_imap_is_none() {
    plain(Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn plain_over_jmap_is_none() {
    plain(Fetch::Jmap).await;
}

/// Bob loses his key: the mail is still stored, with its original, and says what it is.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_mail_for_a_lost_key_is_kept_and_marked() {
    let _one = serial().await;
    let (a, b, c) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let old_bob = Person::open(Who::Bob, b.path(), Fetch::Imap, Submit::Smtp).await;
    alice.knows(&old_bob).await;
    let t = token();
    alice
        .send(None, &[Who::Bob.address()], &[], &format!("Lost-{t}"), &[])
        .await
        .expect("send");
    // Wait until it is there, read by the holder of the key; its id names it for the new device.
    let id = old_bob.wait_for(&format!("Lost-{t}")).await.aliases[0].id.clone();

    let new_bob = Person::open(Who::Bob, c.path(), Fetch::Imap, Submit::Smtp).await;
    fs_messenger_core::Ingest::sync(&new_bob.repo, new_bob.account).await.expect("no error loses it");
    let held = new_bob.find(UNDECRYPTED_MARK).await;
    let mine = held
        .iter()
        .find(|m| m.aliases.iter().any(|x| x.id == id))
        .expect("stored, visibly undecrypted");
    assert_eq!(mine.summary.encryption, Encryption::None);
    let server = raw_by_message_id(Who::Bob, &id).await.unwrap();
    assert_eq!(mine.original.as_ref().unwrap().bytes, server);
    let inspected = new_bob.provider.inspect(&server).await.unwrap();
    assert!(inspected.undecrypted);
}
