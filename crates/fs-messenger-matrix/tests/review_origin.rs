//! Second review, findings 1 and 5: the token stays at the server that issued it, and a password in
//! the homeserver URL is shown nowhere.

mod review_common;

use std::sync::Arc;

use fs_messenger_core::Provider;
use fs_messenger_matrix::store::{FsJournal, Journal, Op};
use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use fs_objects::TursoStore;
use review_common::support::server::{Reply, Server};
use review_common::{account, login, ordinary, support};
use serde_json::json;

fn homeserver(with_sync: bool) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |path| match path {
        p if p.ends_with("/login") => login(),
        p if with_sync && p.contains("/sync") => Reply::json(200, &json!({"next_batch": "s1"})),
        p => ordinary(p),
    }
}

async fn logged_in(server: &Server, store: &Arc<TursoStore>) {
    let password = Password::new("pw");
    let provider = MatrixProvider::login(account(server), &password, store.clone());
    drop(provider.await.expect("login"));
}

#[tokio::test]
async fn a_stored_token_does_not_go_to_another_homeserver() {
    let first = Server::start(homeserver(true)).await;
    let other = Server::start(homeserver(true)).await;
    let store = support::fresh_store().await;
    logged_in(&first, &store).await;

    let restored = MatrixProvider::restore(account(&other), store).await;
    if let Ok(provider) = restored {
        let _ = provider.fetch(None).await;
    }
    assert_eq!(other.heard(), [], "the other server heard something");
}

#[tokio::test]
async fn the_same_homeserver_written_another_way_still_restores() {
    let server = Server::start(homeserver(true)).await;
    let store = support::fresh_store().await;
    logged_in(&server, &store).await;
    let mut again = account(&server);
    again.homeserver = format!("{}/", server.url());
    assert!(MatrixProvider::restore(again, store).await.is_ok());
}

#[tokio::test]
async fn a_session_without_a_stored_homeserver_is_refused_before_any_request() {
    let server = Server::start(homeserver(true)).await;
    let store = support::fresh_store().await;
    logged_in(&server, &store).await;
    let before = server.heard().len();
    // An older draft stored the session alone.
    let journal = FsJournal::open(
        store.clone(),
        format!("matrix-session:{}", review_common::USER),
    );
    let journal = journal.await.unwrap();
    journal
        .append(vec![Op::Remove("homeserver".into())])
        .await
        .unwrap();

    assert!(
        MatrixProvider::restore(account(&server), store)
            .await
            .is_err()
    );
    assert_eq!(server.heard().len(), before, "a request left");
}

#[tokio::test]
async fn a_password_in_the_url_shows_nowhere() {
    let needle = "pw-needle-of-the-url";
    let url = format!("https://alice:{needle}@matrix.example.org");
    let account = MatrixAccount::new(url, review_common::USER);
    let store = support::fresh_store().await;
    let password = Password::new("pw");
    let error = MatrixProvider::login(account.clone(), &password, store).await;
    let error = error.expect_err("user info is refused");
    let shown = format!("{account:?} {account:#?} {error} {error:?}");
    assert!(!shown.contains(needle), "{shown}");
    assert!(shown.contains("user info"), "the reason is named: {shown}");
}
