//! Third review, finding 1 (and 3): `login` and `restore` end within the hard end, also when the SDK's
//! own start-up task is held up by a `429` with a huge `retry_after`. A raw server, not Matrix.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{TOKEN, USER, login, ordinary, support};
use serde_json::json;

/// Login answers, and `account_data` holds up for an hour once `slow` is set.
async fn server(slow: Arc<AtomicBool>) -> Server {
    Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/account_data/") && slow.load(Ordering::SeqCst) => Reply::json(
            429,
            &json!({"errcode":"M_LIMIT_EXCEEDED","error":"slow","retry_after_ms":3_600_000}),
        ),
        p => ordinary(p),
    })
    .await
}

fn quick(server: &Server) -> MatrixAccount {
    let mut account = MatrixAccount::new(server.url(), USER);
    account.request_timeout = Duration::from_secs(1);
    account
}

/// The limit is 8 requests of 1 s; this leaves room for the machine, and none for an hour.
const ENDS_WITHIN: Duration = Duration::from_secs(20);

fn assert_clean(error: &impl std::fmt::Display) {
    let text = error.to_string();
    assert!(!text.contains(TOKEN) && !text.contains("pw-needle"), "{text}");
}

#[tokio::test]
async fn login_ends_when_the_startup_task_is_held_up() {
    let server = server(Arc::new(AtomicBool::new(true))).await;
    let store = support::fresh_store().await;
    let password = Password::new("pw-needle");
    let began = Instant::now();
    let result = tokio::time::timeout(
        ENDS_WITHIN,
        MatrixProvider::login(quick(&server), &password, store),
    )
    .await;
    let answer = result.unwrap_or_else(|_| panic!("login still running after {ENDS_WITHIN:?}"));
    let error = answer.expect_err("a held-up start must be an error");
    assert!(began.elapsed() < ENDS_WITHIN);
    assert_clean(&error);
}

#[tokio::test]
async fn restore_ends_when_the_startup_task_is_held_up() {
    let slow = Arc::new(AtomicBool::new(false));
    let server = server(slow.clone()).await;
    let store = support::fresh_store().await;
    let password = Password::new("pw-needle");
    MatrixProvider::login(quick(&server), &password, store.clone())
        .await
        .unwrap();

    slow.store(true, Ordering::SeqCst);
    let result = tokio::time::timeout(
        ENDS_WITHIN,
        MatrixProvider::restore(quick(&server), store.clone()),
    )
    .await;
    let answer = result.unwrap_or_else(|_| panic!("restore still running after {ENDS_WITHIN:?}"));
    assert_clean(&answer.expect_err("a held-up start must be an error"));

    // Nothing half open: once the server is well again the same store restores.
    slow.store(false, Ordering::SeqCst);
    MatrixProvider::restore(quick(&server), store).await.unwrap();
}
