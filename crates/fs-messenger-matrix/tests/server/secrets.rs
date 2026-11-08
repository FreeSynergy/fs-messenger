//! The binding rule: no access token, passphrase, Olm or Megolm key in any activity, hub path, `Debug`,
//! `Display`, error or log line, or readable in the raw store file. Needles from the real secrets.

use std::path::Path;
use std::sync::Arc;

use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use fs_objects::ActivityFilter;
use fs_objects::traits::{ActivityLog, UnloggedObjects};
use matrix_sdk_crypto::decrypt_room_key_export;

use crate::capture::captured;
use crate::common::{Person, encrypted_room, token};
use crate::http;

/// A journal's record as it stands, through the store's own API.
async fn record(person: &Person, key: &str) -> serde_json::Value {
    let q = fs_objects::Query::new().of_type("freesynergy.net:MatrixStoreSegment");
    let segments = person
        .keys
        .query_unlogged(&q, &["freesynergy.net:MatrixStoreSegment"])
        .await
        .unwrap();
    let mut newest = None;
    let mut segments: Vec<_> = segments.iter().map(|s| &s.content).collect();
    segments.sort_by_key(|c| c["ops"].as_array().map(Vec::len));
    for segment in segments {
        for op in segment["ops"].as_array().unwrap() {
            if op["p"][0] == key {
                newest = Some(op["p"][1].as_str().unwrap().to_owned());
            }
        }
    }
    serde_json::from_str(&newest.expect("the record is in the store")).unwrap()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

/// Every secret the rule names: token, password, passphrase, a Megolm key, an Olm key.
async fn needles(alice: &Person, passphrase: &str, export: &Path) -> Vec<(&'static str, String)> {
    let session = record(alice, "session").await;
    let token = session["access_token"].as_str().unwrap().to_owned();
    assert!(token.len() > 8, "the test found the real token");

    let file = std::fs::File::open(export).unwrap();
    let keys = decrypt_room_key_export(file, passphrase).unwrap();
    let megolm = keys.first().expect("a room key").session_key.to_base64();
    assert!(megolm.len() > 100, "the test found a real Megolm key");

    // The account's secret part as its pickle holds it. Long enough to be nothing else.
    let account = record(alice, "[\"account\"]").await;
    let olm = serde_json::to_string(&account["pickle"]).unwrap();
    assert!(olm.len() > 100, "the test found a real Olm pickle");
    let olm = olm[olm.len() / 2..olm.len() / 2 + 48].to_owned();

    vec![
        ("the access token", token),
        ("the password", alice.password.clone()),
        ("the export passphrase", passphrase.to_owned()),
        ("a Megolm key", megolm),
        ("an Olm key", olm),
    ]
}

#[tokio::test]
#[ignore = "needs a homeserver; see tests/server/main.rs"]
async fn no_secret_in_activity_hub_path_debug_error_log_or_file() {
    let log = captured();
    let alice = Person::new("alice").await;
    let bob = Person::new("bob").await;
    let room = encrypted_room(&alice, &bob).await;
    let text = format!("SecretText-{}", token());
    alice.send(&[&room], &text).await.expect("send");
    bob.wait_for(&text).await;

    let passphrase = format!("export-phrase-{}", token());
    let export = alice.file("export.txt");
    let phrase = Password::new(passphrase.clone());
    alice
        .provider
        .export_room_keys(export.clone(), &phrase)
        .await
        .unwrap();
    let needles = needles(&alice, &passphrase, &export).await;
    let refuse = |place: &str, haystack: &[u8]| {
        for (what, secret) in &needles {
            assert!(!contains(haystack, secret), "{what} is in {place}");
        }
    };

    // Debug, and an error from every way that can fail here.
    let shown = format!("{:?} {:#?} {:?}", alice.provider, alice.provider, phrase);
    refuse("Debug", shown.as_bytes());
    let wrong_password = MatrixProvider::login(
        MatrixAccount::new(http::base(), crate::common::mxid("nobody-here")),
        &Password::new(alice.password.clone()),
        Arc::new(crate::common::open_keys(&alice.file("other.db")).await),
    )
    .await
    .unwrap_err();
    refuse(
        "an error",
        format!("{wrong_password:?} {wrong_password}").as_bytes(),
    );
    let foreign = fs_messenger_core::ProviderCursor(alice.password.clone());
    let refused = fs_messenger_core::Provider::fetch(alice.provider.as_ref(), Some(&foreign));
    let refused = refused.await.unwrap_err();
    refuse(
        "a cursor error",
        format!("{refused:?} {refused}").as_bytes(),
    );

    // Every log line of every crate, down to TRACE.
    let lines = log.lines();
    assert!(
        lines.len() > 1000,
        "the capture works: {} bytes",
        lines.len()
    );
    refuse("the log", &lines);
    assert!(!contains(&lines, &text), "the message text is in the log");

    // The whole activity log of both stores, read through the store's API. `unsynced_only` is what
    // the hub reads, and it is a part of this.
    let key_log = alice
        .keys
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert!(
        key_log.is_empty(),
        "{} activities in the key store",
        key_log.len()
    );
    let core_log = alice
        .messages
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert!(!core_log.is_empty(), "the core's log was read");
    refuse(
        "the activity log",
        serde_json::to_string(&core_log).unwrap().as_bytes(),
    );

    // Nothing of the session or the keys is an object the hub could be handed: all are unlogged.
    let unsynced = ActivityFilter {
        unsynced_only: true,
        ..ActivityFilter::default()
    };
    assert!(alice.keys.activities(&unsynced).await.unwrap().is_empty());

    // The raw files: encrypted, so none of it is readable, nor the room or the words around a key.
    drop(alice.provider);
    drop(alice.repo);
    drop(alice.messages);
    let mut seen = 0;
    for entry in std::fs::read_dir(alice.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.ends_with("export.txt") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        seen += bytes.len();
        refuse(&path.display().to_string(), &bytes);
        for plain in [text.as_str(), room.as_str(), "access_token", "pickle"] {
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
    refuse("the export file", exported.as_bytes());
}
