//! Secrets stay out of `Debug`, logs, the activity log, and readable bytes on disk.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use fs_store_unlogged::ActivityFilter;
use fs_store_unlogged::traits::ActivityLog;
use tracing_subscriber::fmt::MakeWriter;

use crate::common::{Person, encrypted_room, token};
use crate::http;

/// All that is logged in this test binary, from every crate, at every level.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

fn captured() -> &'static Captured {
    static LOG: OnceLock<Captured> = OnceLock::new();
    LOG.get_or_init(|| {
        let log = Captured::default();
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(log.clone())
            .init();
        log
    })
}

/// The access token of this provider, which only its own store holds.
async fn access_token(person: &Person) -> String {
    let store = &person.keys;
    let q = fs_store_unlogged::Query::new().of_type("freesynergy.net:MatrixStoreSegment");
    let segments = fs_store_unlogged::traits::ObjectQuery::query(store.as_ref(), &q)
        .await
        .unwrap();
    let all =
        serde_json::to_string(&segments.iter().map(|s| &s.content).collect::<Vec<_>>()).unwrap();
    let start = all
        .find("access_token")
        .expect("the session is in the store");
    let rest = &all[start..];
    let text: String = rest.chars().skip_while(|c| *c != ':').skip(1).collect();
    let cleaned = text.trim_start_matches(['\\', '"']);
    cleaned
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn no_secret_in_debug_logs_activity_log_or_on_disk() {
    let log = captured();
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;
    let needle = format!("SecretText-{}", token());
    alice.send(&[&room], &needle).await.expect("send");
    bob.wait_for(&needle).await;

    let access = access_token(&alice).await;
    assert!(
        access.len() > 8,
        "the test found the real token: {} chars",
        access.len()
    );
    let passphrase = format!("export-phrase-{}", token());
    let export = alice.file("export.txt");
    let phrase = Password::new(passphrase.clone());
    alice
        .provider
        .export_room_keys(export.clone(), &phrase)
        .await
        .unwrap();

    // Debug.
    let shown = format!("{:?} {:#?} {:?}", alice.provider, alice.provider, phrase);
    for secret in [
        access.as_str(),
        alice.password.as_str(),
        passphrase.as_str(),
    ] {
        assert!(!shown.contains(secret), "Debug shows a secret: {shown}");
    }
    let wrong = MatrixProvider::login(
        MatrixAccount::new(http::base(), crate::common::mxid("nobody-here")),
        &Password::new(alice.password.clone()),
        Arc::new(crate::common::open_keys(&alice.file("other.db")).await),
    )
    .await
    .unwrap_err();
    assert!(!format!("{wrong:?} {wrong}").contains(alice.password.as_str()));

    // Every log line of every crate, down to TRACE.
    let lines = log.0.lock().unwrap().clone();
    assert!(
        lines.len() > 1000,
        "the capture works: {} bytes",
        lines.len()
    );
    for secret in [
        access.as_str(),
        alice.password.as_str(),
        passphrase.as_str(),
    ] {
        assert!(!contains(&lines, secret), "a secret is in the log");
    }

    assert!(!contains(&lines, &needle), "the message text is in the log");

    // The activity log of the key store is empty.
    let entries = alice
        .keys
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert!(
        entries.is_empty(),
        "{} activities in the key store",
        entries.len()
    );

    // The file holds none of it, nor the room, nor a key's name, readable.
    drop(alice.provider);
    drop(alice.repo);
    let mut seen = 0;
    for entry in std::fs::read_dir(alice.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.ends_with("export.txt") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        seen += bytes.len();
        for plain in [
            access.as_str(),
            &needle,
            room.as_str(),
            "access_token",
            "pickle",
        ] {
            assert!(
                !contains(&bytes, plain),
                "{plain} is readable in {}",
                path.display()
            );
        }
    }
    assert!(seen > 4096, "the files were there: {seen} bytes");
    let exported = std::fs::read_to_string(&export).unwrap();
    assert!(
        exported.contains("BEGIN MEGOLM SESSION DATA"),
        "an encrypted export"
    );
}
