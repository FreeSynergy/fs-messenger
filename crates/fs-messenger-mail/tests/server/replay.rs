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
    alice.send(None, &[Who::Bob.address()], &[], &text, &[]).await.expect("send");
    bob.wait_for(&text).await;

    // Replay: fetch everything from the start; each mail is known.
    let mut replayed = 0;
    let mut since = None;
    loop {
        let batch = bob.provider.fetch(since.as_ref()).await.expect("fetch");
        for raw in batch.messages {
            let result = bob.repo.ingest(bob.account, raw).await.expect("ingest");
            assert!(matches!(result, Ingested::Duplicate(_)), "a replay stored something");
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
    assert_eq!(bob.counting.delivered.load(Ordering::SeqCst), 0, "nothing was fetched again");
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
    assert_eq!(bob.counting.delivered.load(Ordering::SeqCst), 1, "only the new mail");
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
