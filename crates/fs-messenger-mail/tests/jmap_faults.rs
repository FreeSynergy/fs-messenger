//! JMAP faults a real server will not produce on request, against a scripted one over real TLS.

#[path = "common/fake_imap.rs"]
mod fake_imap;
#[path = "common/fake_jmap.rs"]
mod fake_jmap;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fake_jmap::{FakeJmap, Script, Stored, start};
use fs_messenger_mail::Password;
use fs_messenger_mail::TlsTrust;
use fs_messenger_mail::fetch::{FetchLimits, HEAD_BYTES, MailCursor, MailFetch, MailboxCursor};
use fs_messenger_mail::jmap::{JmapClient, JmapConfig, JmapFetch};

const LIMITS: FetchLimits = FetchLimits {
    mails: 50,
    bytes: 1024 * 1024,
    batch_bytes: 10 * 1024 * 1024,
};
const LONG: Duration = Duration::from_secs(10);

fn strategy(server: &FakeJmap, idle: Duration) -> JmapFetch {
    let client = JmapClient::new(JmapConfig {
        session_url: format!("https://localhost:{}/jmap/session", server.port),
        user: "me".into(),
        password: Password::new("x"),
        trust: TlsTrust::OnlyThisCertificateForTests(server.certificate.clone()),
        rebase_on_session_origin: false,
        other_credential_origins: vec![],
        idle_timeout: idle,
    })
    .unwrap();
    JmapFetch::new(Arc::new(client))
}

fn mail(id: &str, size: usize) -> Stored {
    let mut bytes = format!("From: a@e\r\nMessage-ID: <{id}@e>\r\n\r\n").into_bytes();
    bytes.resize(size, b'x');
    Stored {
        id: id.into(),
        reported: size as u64,
        bytes,
    }
}

fn script(sizes: &[(&str, usize)]) -> Script {
    Script {
        mails: sizes.iter().map(|(id, size)| mail(id, *size)).collect(),
        ..Script::default()
    }
}

#[tokio::test]
async fn an_error_in_the_sent_mailbox_keeps_what_the_inbox_delivered() {
    let server = start(Script {
        sent_fails: true,
        ..script(&[("m1", 1000)])
    })
    .await;
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &LIMITS)
        .await
        .expect("the inbox was fine");
    assert_eq!(batch.mails.len(), 1);
    assert!(batch.cursor.mailboxes.contains_key("i"));
    assert!(!batch.cursor.mailboxes.contains_key("s"));
}

/// The last review: one download with a total of 60 s. Slow and moving takes longer, and is no fault.
#[tokio::test]
async fn a_slow_download_that_keeps_moving_arrives() {
    let drip = Some(Duration::from_millis(150));
    let server = start(Script {
        drip,
        ..script(&[("m1", 4000)])
    })
    .await;
    let started = Instant::now();
    let batch = strategy(&server, Duration::from_millis(400))
        .fetch(&MailCursor::default(), &LIMITS)
        .await
        .expect("slow is no fault");
    assert_eq!(batch.mails.len(), 1);
    assert!(
        started.elapsed() > Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_stall_in_a_download_ends_in_an_error() {
    let server = start(Script {
        stall_in_body: true,
        ..script(&[("m1", 4000)])
    })
    .await;
    let started = Instant::now();
    let answer = strategy(&server, Duration::from_millis(300))
        .fetch(&MailCursor::default(), &LIMITS)
        .await;
    assert!(answer.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// The budget ends a batch inside one page. What is left waits in the cursor, and is not lost.
#[tokio::test]
async fn the_byte_budget_keeps_what_it_left_in_the_cursor() {
    let server = start(script(&[("m1", 1000), ("m2", 1000), ("m3", 1000)])).await;
    let limits = FetchLimits {
        batch_bytes: 1500,
        ..LIMITS
    };
    let jmap = strategy(&server, LONG);
    let first = jmap.fetch(&MailCursor::default(), &limits).await.unwrap();
    assert_eq!(first.mails.len(), 1);
    assert!(first.more);
    let second = jmap.fetch(&first.cursor, &limits).await.unwrap();
    assert_eq!(second.mails.len(), 1);
    assert!(second.more);
    let third = jmap.fetch(&second.cursor, &limits).await.unwrap();
    assert_eq!(third.mails.len(), 1);
    let order = server.downloads.lock().unwrap().clone();
    assert_eq!(order, ["m1", "m2", "m3"], "each once, none skipped");
}

/// The same in the changes: the state moves on at once, so only the cursor remembers.
#[tokio::test]
async fn the_byte_budget_loses_nothing_from_the_changes() {
    let mut s = script(&[("m1", 1000), ("m2", 1000)]);
    s.created = vec!["m1".into(), "m2".into()];
    let server = start(s).await;
    let mut cursor = MailCursor::default();
    cursor.mailboxes.insert(
        "i".into(),
        MailboxCursor::Jmap {
            state: "s1".into(),
            walking: false,
            anchor: None,
            pending: vec![],
        },
    );
    let limits = FetchLimits {
        batch_bytes: 10,
        ..LIMITS
    };
    let jmap = strategy(&server, LONG);
    let first = jmap.fetch(&cursor, &limits).await.unwrap();
    assert_eq!(first.mails.len(), 1);
    assert!(first.more);
    let second = jmap.fetch(&first.cursor, &limits).await.unwrap();
    assert_eq!(second.mails.len(), 1, "m2 from the cursor");
    assert_eq!(*server.downloads.lock().unwrap(), ["m1", "m2"]);
}

#[tokio::test]
async fn of_a_mail_over_the_limit_only_the_head_is_held_and_it_says_so() {
    let large = usize::try_from(HEAD_BYTES).unwrap() * 2;
    let server = start(script(&[("m1", large)])).await;
    let small = FetchLimits {
        bytes: 1000,
        ..LIMITS
    };
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &small)
        .await
        .unwrap();
    assert_eq!(batch.mails[0].too_large, Some(large as u64));
    assert_eq!(batch.mails[0].bytes.len() as u64, HEAD_BYTES);
}

/// Says small and sends more: cut off at the limit, and it says so. Says huge, sends small: as reported.
#[tokio::test]
async fn a_size_that_lies_either_way_is_a_cut_off_start() {
    let mut says_small = mail("m1", 5000);
    says_small.reported = 10;
    let mut says_huge = mail("m2", 100);
    says_huge.reported = 5_000_000;
    let server = start(Script {
        mails: vec![says_small, says_huge],
        ..Script::default()
    })
    .await;
    let limits = FetchLimits {
        bytes: 1000,
        ..LIMITS
    };
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &limits)
        .await
        .unwrap();
    assert_eq!(batch.mails[0].bytes.len(), 1000);
    assert!(batch.mails[0].too_large.is_some());
    assert_eq!(batch.mails[1].too_large, Some(5_000_000));
}
