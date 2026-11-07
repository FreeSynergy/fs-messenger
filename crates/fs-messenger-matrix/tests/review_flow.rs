//! Second review, finding 2: a plain event cannot take the place of an undecrypted one with the same
//! id. Real provider, real core, a raw server.

mod review_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fs_messenger_core::{Ingest, Messenger, MessengerRepo};
use fs_messenger_matrix::{MatrixProvider, Password, UNDECRYPTED_MARK};
use review_common::support::server::Server;
use review_common::{account, core, encrypted, login, ordinary, plain, support, sync};

/// Every stored message: whether it is undecrypted, and its text.
async fn stored(repo: &MessengerRepo) -> Vec<(bool, String)> {
    let mut all = Vec::new();
    for conversation in repo.conversations(None, false, None).await.unwrap().items {
        for summary in repo.messages(conversation.id, None).await.unwrap().items {
            let message = repo.message(conversation.id, summary.envelope.id).await;
            all.push((summary.undecrypted, message.unwrap().body_text));
        }
    }
    all
}

#[tokio::test]
async fn a_plain_event_does_not_replace_an_undecrypted_one_with_the_same_id() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let server = Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("/sync") => match counter.fetch_add(1, Ordering::SeqCst) {
            0 => sync("s1", &[encrypted("$x")], false, None),
            _ => sync("s2", &[plain("$x", "MALLORY-TEXT")], false, None),
        },
        p => ordinary(p),
    })
    .await;
    let store = support::fresh_store().await;
    let password = Password::new("pw");
    let provider = MatrixProvider::login(account(&server), &password, store);
    let (repo, id) = core(provider.await.unwrap()).await;

    repo.sync(id).await.unwrap();
    assert_eq!(stored(&repo).await, [(true, UNDECRYPTED_MARK.to_owned())]);
    repo.sync(id).await.unwrap();
    assert_eq!(
        stored(&repo).await,
        [(true, UNDECRYPTED_MARK.to_owned())],
        "the placeholder stays"
    );
}
