//! What a homeserver cannot make this provider do. A real SDK client and real HTTP over loopback, to
//! a small server that behaves badly on purpose, plus a second one that must never hear anything.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fs_messenger_core::Provider;
use fs_messenger_matrix::store::{FsCryptoStore, FsJournal, Journal, Op};
use fs_messenger_matrix::{MatrixAccount, MatrixError, MatrixProvider, Password, StoreFail};
use serde_json::json;
use support::server::{Reply, Server};

const USER: &str = "@alice:x.test";
const TOKEN: &str = "token-needle-of-this-test";

fn login_answer(extra: serde_json::Value) -> Reply {
    let mut body = json!({"user_id": USER, "access_token": TOKEN, "device_id": "DEV"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    Reply::json(200, &body)
}

/// What every server answers, so that only the bad answer differs.
fn ordinary(path: &str) -> Reply {
    if path.ends_with("/versions") {
        return Reply::json(200, &json!({"versions": ["v1.11"]}));
    }
    Reply::json(404, &json!({"errcode": "M_NOT_FOUND", "error": "no"}))
}

fn account(server: &Server) -> MatrixAccount {
    let mut account = MatrixAccount::new(server.url(), USER);
    account.request_timeout = Duration::from_secs(1);
    account
}

#[tokio::test]
async fn a_login_answer_cannot_move_the_token_to_another_origin() {
    let thief = Server::start(|_| Reply::json(200, &json!({"next_batch": "stolen"}))).await;
    let well_known = json!({"well_known": {"m.homeserver": {"base_url": thief.url()}}});
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login_answer(well_known.clone()),
        p if p.contains("/sync") => Reply::json(200, &json!({"next_batch": "s1"})),
        p => ordinary(p),
    })
    .await;

    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store).await;
    let provider = provider.expect("login");
    provider.fetch(None).await.expect("sync at the real server");

    assert!(
        server
            .heard()
            .iter()
            .any(|(path, _)| path.contains("/sync"))
    );
    assert!(
        thief.heard().is_empty(),
        "another origin heard: {:?}",
        thief.heard()
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let thief = Server::start(|_| Reply::json(200, &json!({"next_batch": "stolen"}))).await;
    let target = thief.url();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login_answer(json!({})),
        p if p.contains("/sync") => Reply::redirect(&format!("{target}/stolen")),
        p => ordinary(p),
    })
    .await;

    let store = support::fresh_store().await;
    let provider = MatrixProvider::login(account(&server), &Password::new("pw"), store).await;
    let provider = provider.expect("login");
    assert!(
        provider.fetch(None).await.is_err(),
        "the redirect is an error"
    );
    assert!(
        thief.heard().is_empty(),
        "another origin heard: {:?}",
        thief.heard()
    );
}

#[tokio::test]
async fn a_server_that_never_answers_does_not_hang_the_provider() {
    let server = Server::silent().await;
    let store = support::fresh_store().await;
    let started = Instant::now();
    let password = Password::new("pw");
    let login = MatrixProvider::login(account(&server), &password, store);
    let answer = tokio::time::timeout(Duration::from_secs(60), login).await;
    let answer = answer.expect("the SDK's default would retry for ever");
    assert!(answer.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
}

/// Log in, then `/sync` answers `reply` every time. The fetch must end on its own, and soon.
async fn fetch_ends_against(reply: fn() -> Reply) -> Duration {
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login_answer(json!({})),
        p if p.contains("/sync") => reply(),
        p => ordinary(p),
    })
    .await;
    let store = support::fresh_store().await;
    let password = Password::new("pw");
    let provider = MatrixProvider::login(account(&server), &password, store).await;
    let provider = provider.expect("login");

    let started = Instant::now();
    let fetch = provider.fetch(None);
    let answer = tokio::time::timeout(Duration::from_secs(40), fetch).await;
    assert!(answer.expect("a fetch must end on its own").is_err());
    started.elapsed()
}

#[tokio::test]
async fn a_sync_that_never_answers_ends_as_an_error() {
    assert!(fetch_ends_against(Reply::never).await < Duration::from_secs(30));
}

/// The SDK's default retries a 5xx for ever.
#[tokio::test]
async fn a_server_that_always_fails_does_not_keep_the_provider_retrying() {
    let failing = || Reply::json(503, &json!({"errcode": "M_UNKNOWN", "error": "later"}));
    assert!(fetch_ends_against(failing).await < Duration::from_secs(30));
}

/// A server names how long to wait, and the SDK believes it.
#[tokio::test]
async fn a_server_cannot_make_the_provider_wait_an_hour() {
    let wait = || {
        let body =
            json!({"errcode": "M_LIMIT_EXCEEDED", "error": "wait", "retry_after_ms": 3_600_000});
        Reply::json(429, &body)
    };
    assert!(fetch_ends_against(wait).await < Duration::from_secs(30));
}

#[tokio::test]
async fn a_plain_store_is_refused_before_anything_is_sent() {
    let server = Server::start(|_| Reply::json(500, &json!({}))).await;
    let dir = tempfile::tempdir().unwrap();
    let options = fs_store::OpenOptions {
        allow_sensitive_unencrypted: true,
        ..fs_store::OpenOptions::default()
    };
    let opted_out = fs_store::TursoStore::open_with(dir.path().join("plain.db"), options).await;
    let in_memory = fs_store::TursoStore::open_in_memory().await;
    for plain in [opted_out.unwrap(), in_memory.unwrap()] {
        let plain = Arc::new(plain);
        let password = Password::new("pw");
        let refused = MatrixProvider::login(account(&server), &password, plain.clone()).await;
        let refused = refused.unwrap_err();
        assert!(
            matches!(refused, MatrixError::Store(StoreFail::NotEncrypted)),
            "{refused}"
        );
        let refused = MatrixProvider::restore(account(&server), plain)
            .await
            .unwrap_err();
        assert!(
            matches!(refused, MatrixError::Store(StoreFail::NotEncrypted)),
            "{refused}"
        );
    }
    assert!(server.heard().is_empty(), "{:?}", server.heard());
}

/// A parser's message quotes what it read: `invalid type: string "..."`.
#[tokio::test]
async fn a_damaged_record_does_not_quote_its_key_in_the_error() {
    let needle = "olm-key-needle-do-not-print";
    let store = support::fresh_store().await;
    let journal = FsJournal::open(store.clone(), "crypto").await.unwrap();
    let value = serde_json::to_string(needle).unwrap();
    let key = serde_json::to_string(&["account"]).unwrap();
    journal
        .append(vec![Op::Put(key, value.into())])
        .await
        .unwrap();

    let failed = FsCryptoStore::open(Arc::new(journal)).await.unwrap_err();
    let mut shown = format!("{failed} {failed:?}");
    shown.push_str(&MatrixError::from(failed).to_string());
    assert!(
        shown.contains("line"),
        "the error still says something: {shown}"
    );
    assert!(!shown.contains(needle), "{shown}");
}
