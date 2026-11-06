//! A replay stores nothing twice, and a restart resumes from the cursor.

use std::sync::atomic::Ordering;

use fs_messenger_core::{Ingest, Ingested, Provider};

use crate::common::{Fetch, Person, Submit, Who, serial, smtp_raw, token};

async fn replay_and_restart(fetch: Fetch) {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), fetch, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), fetch, Submit::Smtp).await;
    alice.knows(&bob).await;
    let t = token();
    let text = format!("Replay-{t}");
    alice
        .send(None, &[Who::Bob.address()], &[], &text, &[])
        .await
        .expect("send");
    bob.wait_for(&text).await;

    // Replay: fetch everything from the start; each mail is known.
    let mut replayed = 0;
    let mut since = None;
    loop {
        let batch = bob.provider.fetch(since.as_ref()).await.expect("fetch");
        for raw in batch.messages {
            let result = bob.repo.ingest(bob.account, raw).await.expect("ingest");
            assert!(
                matches!(result, Ingested::Duplicate(_)),
                "a replay stored something"
            );
            replayed += 1;
        }
        since = batch.next;
        if !batch.more {
            break;
        }
    }
    assert!(replayed >= 1);
    assert_eq!(bob.find(&text).await.len(), 1);

    // Restart: a new core and provider on the same disk.
    let dir = bob.dir.clone();
    drop(bob);
    let bob = Person::open(Who::Bob, &dir, fetch, Submit::Smtp).await;
    let report = bob.repo.sync(bob.account).await.expect("sync");
    assert_eq!(report.stored, 0);
    assert_eq!(
        bob.counting.delivered.load(Ordering::SeqCst),
        0,
        "nothing was fetched again"
    );
    assert_eq!(bob.find(&text).await.len(), 1);

    // And it still moves on: the next mail arrives, and only that one.
    let next = format!("After-{t}");
    let raw = format!(
        "From: {}\r\nTo: {}\r\nSubject: x\r\nMessage-ID: <{t}-next@msgtest.test>\r\n\r\n{next}\r\n",
        Who::Alice.address(),
        Who::Bob.address()
    );
    smtp_raw(Who::Alice, Who::Bob, raw.as_bytes()).await;
    bob.wait_for(&next).await;
    assert_eq!(
        bob.counting.delivered.load(Ordering::SeqCst),
        1,
        "only the new mail"
    );
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn over_imap() {
    replay_and_restart(Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn over_jmap() {
    replay_and_restart(Fetch::Jmap).await;
}

/// A stranger claims the `Message-ID` of a mail bob already has. His mail must still be stored, and
/// the first one must stay as it is.
async fn forged_message_id(fetch: Fetch) {
    let _one = serial().await;
    let b = tempfile::tempdir().unwrap();
    let bob = Person::open(Who::Bob, b.path(), fetch, Submit::Smtp).await;
    let t = token();
    // Different subjects: Stalwart itself drops a second delivery with the same id and the same subject
    // (observed on 1.0.3), so that one never reaches any client. Another subject is what an attacker
    // would send anyway, and it does reach the mailbox.
    let mail = |body: &str| {
        format!(
            "From: {}\r\nTo: {}\r\nSubject: {body}\r\nMessage-ID: <{t}-same@msgtest.test>\r\n\r\n{body}\r\n",
            Who::Alice.address(),
            Who::Bob.address()
        )
    };
    let (first, second) = (format!("First-{t}"), format!("Second-{t}"));
    smtp_raw(Who::Alice, Who::Bob, mail(&first).as_bytes()).await;
    bob.wait_for(&first).await;
    smtp_raw(Who::Bob, Who::Bob, mail(&second).as_bytes()).await;
    bob.wait_for(&second).await;
    assert_eq!(bob.find(&first).await.len(), 1, "the first one stays");
    assert_eq!(
        bob.find(&second).await.len(),
        1,
        "the forged one is not suppressed"
    );
}

/// Worse: the stranger also writes `From: alice` and takes the id of a mail alice sent. In her inbox.
/// The core would match this against her receipt; the provider withholds the link.
async fn forged_own_sender(fetch: Fetch) {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), fetch, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), fetch, Submit::Smtp).await;
    alice.knows(&bob).await;
    let t = token();
    let text = format!("Genuine-{t}");
    let sent = alice
        .send(None, &[Who::Bob.address()], &[], &text, &[])
        .await
        .expect("send");
    let id = crate::common::message_id_of(&alice.message(sent).await);
    let forged = format!(
        "From: {}\r\nTo: {}\r\nSubject: x\r\nMessage-ID: <{id}>\r\n\r\nClaimed-{t}\r\n",
        Who::Alice.address(),
        Who::Alice.address()
    );
    smtp_raw(Who::Bob, Who::Alice, forged.as_bytes()).await;
    let stored = alice.wait_for(&format!("Claimed-{t}")).await;
    assert!(!stored.outgoing, "not mine because it says so");
    assert_ne!(stored.id(), sent);
    let own = alice.find(&text).await;
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].id(), sent, "my message is untouched");
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_forged_own_sender_with_a_known_id_is_stored_over_imap() {
    forged_own_sender(Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_forged_own_sender_with_a_known_id_is_stored_over_jmap() {
    forged_own_sender(Fetch::Jmap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_forged_message_id_does_not_suppress_a_mail_over_imap() {
    forged_message_id(Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_forged_message_id_does_not_suppress_a_mail_over_jmap() {
    forged_message_id(Fetch::Jmap).await;
}

/// A legal `|` in a `Message-ID` costs neither the mail nor its reply.
async fn pipe_in_an_id(fetch: Fetch) {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), fetch, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), fetch, Submit::Smtp).await;
    bob.knows(&alice).await;
    let t = token();
    let (from, to) = (Who::Alice.address(), Who::Bob.address());
    let first = format!("Pipe-{t}");
    let raw = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: p\r\nMessage-ID: <{t}|a@msgtest.test>\r\n\r\n{first}\r\n"
    );
    smtp_raw(Who::Alice, Who::Bob, raw.as_bytes()).await;
    let parent = bob.wait_for(&first).await;
    let second = format!("Pipe2-{t}");
    let raw = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: q\r\nMessage-ID: <{t}|b@msgtest.test>\r\n\
         In-Reply-To: <{t}|a@msgtest.test>\r\nReferences: <{t}|a@msgtest.test>\r\n\r\n{second}\r\n"
    );
    smtp_raw(Who::Alice, Who::Bob, raw.as_bytes()).await;
    let child = bob.wait_for(&second).await;
    assert_eq!(
        child.summary.envelope.in_reply_to,
        Some(parent.id().to_string())
    );
    assert_eq!(
        child.summary.envelope.context,
        parent.summary.envelope.context
    );
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_pipe_in_a_message_id_is_stored_over_imap() {
    pipe_in_an_id(Fetch::Imap).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_pipe_in_a_message_id_is_stored_over_jmap() {
    pipe_in_an_id(Fetch::Jmap).await;
}
