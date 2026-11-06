//! JMAP listing under deletion, and mail over the size limit, on the real server.

use fs_messenger_mail::fetch::{FetchLimits, FetchedMail, MailCursor, MailFetch};
use fs_messenger_mail::jmap::JmapFetch;

use crate::common::{Who, contains, serial, smtp_raw, token};
use crate::env::destroy;
use crate::wire::{imap_fetch, jmap_client};

const ONE: FetchLimits = FetchLimits {
    mails: 1,
    bytes: usize::MAX,
};

fn mail(t: &str, n: u32, pad: usize) -> Vec<u8> {
    format!(
        "From: {}\r\nTo: {}\r\nSubject: m{n}\r\nMessage-ID: <{t}-{n}@msgtest.test>\r\n\r\nM{n}-{t}\r\n{}\r\n",
        Who::Alice.address(),
        Who::Bob.address(),
        "x".repeat(pad)
    )
    .into_bytes()
}

/// Waits until bob's mailbox shows `needle`, so that the order on the server is settled.
async fn delivered(needle: &str) {
    for _ in 0..60 {
        if crate::common::raw_mails(Who::Bob)
            .await
            .iter()
            .any(|m| contains(&m.bytes, needle))
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("{needle} did not arrive");
}

fn has(mails: &[FetchedMail], needle: &str) -> bool {
    mails.iter().any(|m| contains(&m.bytes, needle))
}

/// The email id in a JMAP `stable_id`.
fn email_id(mail: &FetchedMail) -> String {
    mail.stable_id.rsplit('/').next().unwrap().to_string()
}

async fn three_mails(t: &str) {
    for n in 1..=3 {
        smtp_raw(Who::Alice, Who::Bob, &mail(t, n, 0)).await;
        delivered(&format!("M{n}-{t}")).await;
    }
}

/// A walk until the end, with `after_first` run after the first page.
async fn walk(
    fetch: &JmapFetch,
    seen: &mut Vec<FetchedMail>,
    deleted_after: usize,
    to_delete: usize,
) {
    let mut cursor = MailCursor::default();
    for page in 0..12 {
        let batch = fetch.fetch(&cursor, &ONE).await.expect("fetch");
        seen.extend(batch.mails);
        cursor = batch.cursor;
        if page + 1 == deleted_after {
            destroy(Who::Bob, &email_id(&seen[to_delete])).await;
        }
        if !batch.more {
            return;
        }
    }
    panic!("the walk does not end");
}

/// The anchor itself goes: the walk starts over, and nothing is lost.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn deleting_the_anchor_between_pages_loses_no_mail() {
    let _one = serial().await;
    let t = token();
    three_mails(&t).await;
    let fetch = JmapFetch::new(jmap_client(Who::Bob));
    let mut seen = vec![];
    walk(&fetch, &mut seen, 1, 0).await;
    assert!(has(&seen, &format!("M2-{t}")), "m2 must arrive");
    assert!(has(&seen, &format!("M3-{t}")), "m3 must arrive");
}

/// An earlier mail goes while the anchor stays: a position would now skip one.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn deleting_an_earlier_mail_between_pages_loses_no_mail() {
    let _one = serial().await;
    let t = token();
    three_mails(&t).await;
    let fetch = JmapFetch::new(jmap_client(Who::Bob));
    let mut seen = vec![];
    walk(&fetch, &mut seen, 2, 0).await;
    assert!(has(&seen, &format!("M2-{t}")));
    assert!(has(&seen, &format!("M3-{t}")), "m3 must arrive");
}

async fn over_the_limit(fetch: &dyn MailFetch) {
    let t = token();
    smtp_raw(Who::Alice, Who::Bob, &mail(&t, 1, 200_000)).await;
    smtp_raw(Who::Alice, Who::Bob, &mail(&t, 2, 0)).await;
    delivered(&format!("M2-{t}")).await;
    delivered(&format!("M1-{t}")).await;
    let limits = FetchLimits {
        mails: 50,
        bytes: 100_000,
    };
    let batch = fetch
        .fetch(&MailCursor::default(), &limits)
        .await
        .expect("fetch");
    assert!(
        has(&batch.mails, &format!("M2-{t}")),
        "the small one arrives"
    );
    assert!(
        !has(&batch.mails, &format!("M1-{t}")),
        "the large one is not downloaded"
    );
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_mail_over_the_limit_is_passed_over_imap() {
    let _one = serial().await;
    over_the_limit(&imap_fetch(Who::Bob)).await;
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_mail_over_the_limit_is_passed_over_jmap() {
    let _one = serial().await;
    over_the_limit(&JmapFetch::new(jmap_client(Who::Bob))).await;
}
