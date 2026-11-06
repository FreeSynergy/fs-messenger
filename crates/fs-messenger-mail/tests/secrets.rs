//! A secret reaches no activity, no event, no `Debug`, no `Display`, no error, and not the raw file of an
//! encrypted store. Every needle check has a control that shows it can find the needle.

#[path = "common/forge.rs"]
mod forge;

use std::sync::Arc;

use forge::{files_of, holds_any, key, secret_needles};
use fs_messenger_mail::fetch::{DEFAULT_IDLE_TIMEOUT, ImapConfig};
use fs_messenger_mail::jmap::JmapConfig;
use fs_messenger_mail::keys::{FsKeyStore, KeySource, KeyStore, PeerKey};
use fs_messenger_mail::openpgp::{generate_key, unix_now};
use fs_messenger_mail::submit::SmtpConfig;
use fs_messenger_mail::{Endpoint, Password, TlsTrust};
use fs_store::{
    ActivityFilter, Cipher, EncryptionKey, Envelope, OpenOptions, StaticKeyProvider, Store,
    StoredObject, TursoStore,
};

const ME: &str = "me@example.org";
const PEER: &str = "peer@example.org";

fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

/// The whole log, as far as the store's API shows it: `Debug` and JSON of every activity.
async fn whole_log(store: &TursoStore) -> Vec<u8> {
    let log = Store::activities(store, &ActivityFilter::default())
        .await
        .unwrap();
    let mut all = format!("{log:?}").into_bytes();
    all.extend(serde_json::to_vec(&log).unwrap());
    all
}

#[tokio::test]
async fn the_secret_key_is_in_no_activity_and_no_event_and_public_peers_may_be() {
    let store = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let mut events = Store::subscribe(store.as_ref());
    let keys = FsKeyStore::open(store.clone(), "me");
    let (first, first_rpgp) = key(ME);
    let (second, second_rpgp) = key(ME);
    keys.put_own_key(ME, &first).await.unwrap();
    keys.put_own_key(ME, &second).await.unwrap();
    assert_eq!(
        keys.own_key(ME).await.unwrap().unwrap().fingerprint(),
        second.fingerprint()
    );
    let log = Store::activities(store.as_ref(), &ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(log.len(), 0, "own keys write no activity at all");
    assert!(events.try_recv().is_err(), "and no event");

    let peer = generate_key(PEER).unwrap().public();
    let record = PeerKey::new(PEER, &peer, KeySource::Person, unix_now()).unwrap();
    keys.put_peer(&record).await.unwrap();
    let log = whole_log(&store).await;
    assert!(
        holds_any(&log, &[record.cert.clone().into_bytes()]),
        "a public key may be logged"
    );
    for needles in [secret_needles(&first_rpgp), secret_needles(&second_rpgp)] {
        assert!(!holds_any(&log, &needles), "a secret in the log");
    }
}

/// The control: the same content on the logged path IS in the log, so the check above can see it.
#[tokio::test]
async fn the_log_check_can_find_a_secret_on_the_logged_path() {
    use base64::Engine as _;
    let store = TursoStore::open_in_memory().await.unwrap();
    let (_, rpgp) = key(ME);
    let needles = secret_needles(&rpgp);
    let whole = std::str::from_utf8(&needles[2]).unwrap();
    assert!(
        base64::engine::general_purpose::STANDARD
            .decode(whole)
            .is_ok()
    );
    let object = StoredObject::new(
        Envelope::new("t", "n"),
        "application/json",
        serde_json::json!({"secret": whole}),
    );
    Store::put(&store, object, "me").await.unwrap();
    assert!(holds_any(&whole_log(&store).await, &needles));
}

#[tokio::test]
async fn the_raw_file_of_an_encrypted_store_holds_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("k.db"), encrypted())
            .await
            .unwrap(),
    );
    let keys = FsKeyStore::open_turso(store.clone(), "me").unwrap();
    let (secret, rpgp) = key(ME);
    keys.put_own_key(ME, &secret).await.unwrap();
    let needles = secret_needles(&rpgp);
    assert!(!holds_any(&files_of(dir.path()), &needles), "while open");
    drop(keys);
    drop(store);
    assert!(!holds_any(&files_of(dir.path()), &needles), "after closing");
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("k.db"), encrypted())
            .await
            .unwrap(),
    );
    let keys = FsKeyStore::open_turso(store, "me").unwrap();
    assert_eq!(
        keys.own_key(ME).await.unwrap().unwrap().fingerprint(),
        secret.fingerprint()
    );
}

/// The control, and the opt-out named in `DESIGN.md`: without encryption the file does show it.
#[tokio::test]
async fn the_file_check_can_find_a_secret_in_a_store_that_opted_out() {
    let dir = tempfile::tempdir().unwrap();
    let options = OpenOptions {
        allow_sensitive_unencrypted: true,
        ..OpenOptions::default()
    };
    let store = Arc::new(
        TursoStore::open_with(dir.path().join("k.db"), options)
            .await
            .unwrap(),
    );
    let keys = FsKeyStore::open(store.clone(), "me");
    let (secret, rpgp) = key(ME);
    keys.put_own_key(ME, &secret).await.unwrap();
    assert!(holds_any(&files_of(dir.path()), &secret_needles(&rpgp)));
    assert!(
        FsKeyStore::open_turso(store, "me").is_err(),
        "and a program binary refuses it"
    );
}

#[tokio::test]
async fn a_plain_store_refuses_a_secret_key() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Arc::new(TursoStore::open(dir.path().join("p.db")).await.unwrap());
    let (secret, rpgp) = key(ME);
    assert!(FsKeyStore::open_turso(plain.clone(), "me").is_err());
    let keys = FsKeyStore::open(plain, "me");
    let error = keys.put_own_key(ME, &secret).await.expect_err("refused");
    let needles = secret_needles(&rpgp);
    assert!(!holds_any(
        format!("{error} {error:?}").as_bytes(),
        &needles
    ));
    assert!(!holds_any(&files_of(dir.path()), &needles));
}

#[test]
fn neither_debug_nor_display_prints_a_secret() {
    let (secret, rpgp) = key(ME);
    let needles = secret_needles(&rpgp);
    let shown = format!("{secret} {secret:?} {secret:#?} {:?}", Some(&secret));
    assert!(!holds_any(shown.as_bytes(), &needles));
    // Any other printed form, a list of numbers for one, matches no needle. A redacted form is short.
    for form in [
        format!("{secret}"),
        format!("{secret:?}"),
        format!("{secret:#?}"),
    ] {
        assert!(form.contains("redacted"), "{form}");
        assert!(form.len() < 120, "{form}");
    }
}

#[test]
fn no_configuration_prints_a_password() {
    const NEEDLE: &str = "needle-password-4711";
    let password = Password::new(NEEDLE);
    let endpoint = Endpoint {
        host: "h".into(),
        port: 1,
        trust: TlsTrust::SystemRoots,
    };
    let imap = ImapConfig {
        endpoint: endpoint.clone(),
        user: "u".into(),
        password: password.clone(),
        idle_timeout: DEFAULT_IDLE_TIMEOUT,
    };
    let smtp = SmtpConfig {
        endpoint,
        user: "u".into(),
        password: password.clone(),
    };
    let jmap = JmapConfig {
        session_url: "https://h/".into(),
        user: "u".into(),
        password: password.clone(),
        trust: TlsTrust::SystemRoots,
        rebase_on_session_origin: false,
        other_credential_origins: vec![],
        idle_timeout: DEFAULT_IDLE_TIMEOUT,
    };
    let shown = format!("{password} {password:?} {imap:?} {smtp:?} {jmap:?} {imap:#?}");
    assert!(!shown.contains(NEEDLE), "{shown}");
}
