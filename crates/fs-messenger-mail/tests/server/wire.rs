//! Reaching the server directly: the strategies, and raw bytes past the core.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fs_messenger_core::Message;
use fs_messenger_mail::fetch::{
    DEFAULT_IDLE_TIMEOUT, FetchLimits, FetchedMail, ImapConfig, ImapFetch, MailCursor, MailFetch,
};
use fs_messenger_mail::jmap::{JmapClient, JmapConfig};
use fs_messenger_mail::submit::{MailEnvelope, MailSubmit, SmtpConfig, SmtpSubmit};
use fs_messenger_mail::{Endpoint, TlsTrust};

use crate::env::{Env, Who, server_certificate};

fn trust() -> TlsTrust {
    TlsTrust::OnlyThisCertificateForTests(server_certificate().to_string())
}

pub fn imap_fetch(who: Who) -> ImapFetch {
    let env = Env::load();
    ImapFetch::new(ImapConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: env.port("FS_MSGTEST_IMAP_PORT"),
            trust: trust(),
        },
        user: who.address(),
        password: who.password(),
        idle_timeout: DEFAULT_IDLE_TIMEOUT,
    })
}

pub fn smtp_submit(who: Who) -> SmtpSubmit {
    let env = Env::load();
    SmtpSubmit::new(SmtpConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: env.port("FS_MSGTEST_SMTP_PORT"),
            trust: trust(),
        },
        user: who.address(),
        password: who.password(),
    })
}

pub fn jmap_client(who: Who) -> Arc<JmapClient> {
    let env = Env::load();
    let session_url = format!(
        "https://localhost:{}/jmap/session",
        env.get("FS_MSGTEST_HTTPS_PORT")
    );
    Arc::new(
        JmapClient::new(JmapConfig {
            session_url,
            user: who.address(),
            password: who.password(),
            trust: trust(),
            rebase_on_session_origin: true,
            other_credential_origins: vec![],
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        })
        .expect("jmap client"),
    )
}

/// Every mail in `who`'s mailboxes, as raw bytes, read with a fresh IMAP connection.
pub async fn raw_mails(who: Who) -> Vec<FetchedMail> {
    let fetch = imap_fetch(who);
    let mut cursor = MailCursor::default();
    let mut all = vec![];
    loop {
        let limits = FetchLimits {
            mails: 50,
            bytes: usize::MAX,
            batch_bytes: u64::MAX,
        };
        let batch = fetch.fetch(&cursor, &limits).await.expect("raw fetch");
        all.extend(batch.mails);
        cursor = batch.cursor;
        if !batch.more {
            return all;
        }
    }
}

pub fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

/// The mail on the server that carries this `Message-ID`, read separately from the provider.
pub async fn raw_by_message_id(who: Who, id: &str) -> Option<Vec<u8>> {
    let header = format!("<{id}>");
    raw_mails(who)
        .await
        .into_iter()
        .find(|m| contains(&m.bytes, &header))
        .map(|m| m.bytes)
}

/// The `Message-ID` the core holds of a message: what the sender chose, or our own receipt.
pub fn message_id_of(m: &Message) -> String {
    m.links
        .iter()
        .chain(&m.receipts)
        .find(|x| x.is_sender_chosen())
        .expect("a Message-ID")
        .id
        .clone()
}

/// A unique token, so that a test finds its own mail in shared mailboxes.
pub fn token() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("tok{now}x{}", N.fetch_add(1, Ordering::SeqCst))
}

/// Submit bytes directly, past the core and past every check: what a stranger's mail looks like.
pub async fn smtp_raw(who: Who, to: Who, raw: &[u8]) {
    let envelope = MailEnvelope {
        from: who.address(),
        recipients: vec![to.address()],
    };
    smtp_submit(who).submit(&envelope, raw).await.expect("smtp");
}
