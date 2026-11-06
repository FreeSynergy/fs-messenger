//! IMAP faults a real server will not produce on request, against a scripted one over real TLS.

#[path = "common/fake_imap.rs"]
mod fake_imap;

use std::time::{Duration, Instant};

use fake_imap::{FakeImap, MAIL, Script, Stored, start};
use fs_messenger_mail::fetch::{
    FetchLimits, HEAD_BYTES, ImapConfig, ImapFetch, MailCursor, MailFetch, MailboxCursor,
};
use fs_messenger_mail::{Endpoint, Password, TlsTrust};

const LIMITS: FetchLimits = FetchLimits {
    mails: 50,
    bytes: 1024 * 1024,
    batch_bytes: 10 * 1024 * 1024,
};

fn strategy(server: &FakeImap, idle: Duration) -> ImapFetch {
    ImapFetch::new(ImapConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: server.port,
            trust: TlsTrust::OnlyThisCertificateForTests(server.certificate.clone()),
        },
        user: "me".into(),
        password: Password::new("x"),
        idle_timeout: idle,
    })
}

const LONG: Duration = Duration::from_secs(10);

fn mails(sizes: &[usize]) -> Script {
    let mut uid = 0;
    Script {
        mails: sizes
            .iter()
            .map(|size| {
                uid += 1;
                let mut bytes = format!("From: a@e\r\nMessage-ID: <{uid}@e>\r\n\r\n").into_bytes();
                bytes.resize(*size, b'x');
                Stored {
                    uid,
                    reported: *size as u64,
                    bytes,
                }
            })
            .collect(),
        ..Script::default()
    }
}

fn last_uid(cursor: &MailCursor) -> u32 {
    match cursor.mailboxes.get("INBOX") {
        Some(MailboxCursor::Imap { last_uid, .. }) => *last_uid,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_error_in_the_sent_mailbox_keeps_what_the_inbox_delivered() {
    let server = start(Script::one_mail()).await;
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &LIMITS)
        .await
        .expect("the inbox was fine");
    assert_eq!(batch.mails.len(), 1);
    assert_eq!(batch.mails[0].bytes, MAIL);
    assert!(
        batch.cursor.mailboxes.contains_key("INBOX"),
        "the inbox moves on"
    );
    assert!(
        !batch.cursor.mailboxes.contains_key("Bad"),
        "the sent mailbox stays"
    );
}

#[tokio::test]
async fn a_silent_server_ends_in_an_error_not_a_hang() {
    let server = start(Script {
        silent: true,
        ..Script::default()
    })
    .await;
    let imap = strategy(&server, Duration::from_millis(200));
    let started = Instant::now();
    assert!(imap.fetch(&MailCursor::default(), &LIMITS).await.is_err());
    assert!(imap.store_sent(b"x").await.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// Standing still in the middle of a mail is what ends a call.
#[tokio::test]
async fn a_stall_in_the_middle_of_a_mail_ends_in_an_error() {
    let server = start(Script {
        stall_in_body: true,
        ..mails(&[4000])
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

/// A slow mail that keeps moving takes longer than the idle timeout, and is no fault.
#[tokio::test]
async fn a_slow_mail_that_keeps_moving_arrives() {
    let drip = Some(Duration::from_millis(150));
    let server = start(Script {
        drip,
        ..mails(&[4000])
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

/// By the sizes the server reports, as far as the budget goes; the rest waits for the next call.
#[tokio::test]
async fn the_byte_budget_ends_a_batch_and_the_next_goes_on() {
    let server = start(mails(&[1000, 1000, 1000])).await;
    let limits = FetchLimits {
        batch_bytes: 2500,
        ..LIMITS
    };
    let imap = strategy(&server, LONG);
    let first = imap.fetch(&MailCursor::default(), &limits).await.unwrap();
    assert_eq!(first.mails.len(), 2);
    assert!(first.more, "the third one waits");
    assert_eq!(
        last_uid(&first.cursor),
        2,
        "the cursor stops at what was taken"
    );
    let second = imap.fetch(&first.cursor, &limits).await.unwrap();
    assert_eq!(second.mails.len(), 1);
    assert!(!second.more);
    assert_eq!(last_uid(&second.cursor), 3);
}

/// One mail larger than the budget still passes, or the account would stand still for ever.
#[tokio::test]
async fn at_least_one_mail_passes_whatever_the_budget() {
    let server = start(mails(&[1000, 1000])).await;
    let limits = FetchLimits {
        batch_bytes: 10,
        ..LIMITS
    };
    let first = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &limits)
        .await
        .unwrap();
    assert_eq!(first.mails.len(), 1);
    assert!(first.more);
}

#[tokio::test]
async fn the_highest_uid_is_no_overflow_and_asks_for_nothing() {
    let server = start(Script::one_mail()).await;
    let mut cursor = MailCursor::default();
    cursor.mailboxes.insert(
        "INBOX".into(),
        MailboxCursor::Imap {
            uid_validity: 1,
            last_uid: u32::MAX,
        },
    );
    let batch = strategy(&server, LONG)
        .fetch(&cursor, &LIMITS)
        .await
        .expect("no panic");
    assert!(batch.mails.is_empty());
    let commands = server.commands.lock().unwrap();
    assert!(
        !commands.iter().any(|c| c.contains("SEARCH")),
        "{commands:?}"
    );
}

#[tokio::test]
async fn of_a_mail_over_the_limit_only_the_head_is_asked_for_and_it_says_so() {
    let large = usize::try_from(HEAD_BYTES).unwrap() * 2;
    let server = start(mails(&[large])).await;
    let small = FetchLimits {
        bytes: 1000,
        ..LIMITS
    };
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &small)
        .await
        .unwrap();
    assert_eq!(batch.mails.len(), 1);
    assert_eq!(batch.mails[0].too_large, Some(large as u64));
    assert_eq!(
        batch.mails[0].bytes.len() as u64,
        HEAD_BYTES,
        "not the rest"
    );
    assert_eq!(last_uid(&batch.cursor), 1);
    let commands = server.commands.lock().unwrap();
    assert!(
        !commands.iter().any(|c| c.contains("BODY.PEEK[])")),
        "{commands:?}"
    );
}

/// A server that reports huge and sends small: still only a start, as far as we can tell.
#[tokio::test]
async fn a_size_reported_huge_and_sent_small_is_said_as_reported() {
    let mut script = Script::one_mail();
    script.mails[0].reported = 5_000_000;
    let server = start(script).await;
    let batch = strategy(&server, LONG)
        .fetch(&MailCursor::default(), &LIMITS)
        .await
        .unwrap();
    assert_eq!(batch.mails[0].too_large, Some(5_000_000));
    assert_eq!(batch.mails[0].bytes, MAIL);
}
