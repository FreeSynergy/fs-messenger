//! IMAP faults a real server will not produce on request, against a scripted one over real TLS.

#[path = "common/fake_imap.rs"]
mod fake_imap;

use std::time::{Duration, Instant};

use fake_imap::{FakeImap, MAIL, start};
use fs_messenger_mail::fetch::{
    FetchLimits, ImapConfig, ImapFetch, MailCursor, MailFetch, MailboxCursor,
};
use fs_messenger_mail::{Endpoint, Password, TlsTrust};

const LIMITS: FetchLimits = FetchLimits {
    mails: 50,
    bytes: 1024 * 1024,
};

fn strategy(server: &FakeImap, timeout: Duration) -> ImapFetch {
    ImapFetch::new(ImapConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: server.port,
            trust: TlsTrust::OnlyThisCertificateForTests(server.certificate.clone()),
        },
        user: "me".into(),
        password: Password::new("x"),
        timeout,
    })
}

#[tokio::test]
async fn an_error_in_the_sent_mailbox_keeps_what_the_inbox_delivered() {
    let server = start(false).await;
    let batch = strategy(&server, Duration::from_secs(10))
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
    let server = start(true).await;
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

#[tokio::test]
async fn the_highest_uid_is_no_overflow_and_asks_for_nothing() {
    let server = start(false).await;
    let mut cursor = MailCursor::default();
    cursor.mailboxes.insert(
        "INBOX".into(),
        MailboxCursor::Imap {
            uid_validity: 1,
            last_uid: u32::MAX,
        },
    );
    let batch = strategy(&server, Duration::from_secs(10))
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
async fn a_mail_over_the_limit_is_not_downloaded_and_the_cursor_passes_it() {
    let server = start(false).await;
    let small = FetchLimits {
        mails: 50,
        bytes: 4,
    };
    let batch = strategy(&server, Duration::from_secs(10))
        .fetch(&MailCursor::default(), &small)
        .await
        .unwrap();
    assert!(batch.mails.is_empty());
    let expected = MailboxCursor::Imap {
        uid_validity: 1,
        last_uid: 7,
    };
    assert_eq!(batch.cursor.mailboxes.get("INBOX"), Some(&expected));
    let commands = server.commands.lock().unwrap();
    assert!(!commands.iter().any(|c| c.contains("BODY")), "{commands:?}");
}
