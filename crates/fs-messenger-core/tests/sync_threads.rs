//! Sync with a provider, and threading of messages that arrive in any order.

mod common;

use common::{TestProvider, account, alias, conversation_of, raw, repo_over, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, FetchBatch, Ingest, Ingested, Messenger, ProviderCursor,
};
use fs_objects::{Query, Store, TursoStore};
use std::sync::Arc;

#[tokio::test]
async fn sync_moves_the_cursor_after_storing_and_dedupes_a_repeated_batch() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &[]).await;
    provider.batches.lock().unwrap().extend([
        FetchBatch {
            messages: vec![raw("m1", "t", "1"), raw("m2", "t", "2")],
            next: Some(ProviderCursor("c1".into())),
            more: true,
        },
        FetchBatch {
            messages: vec![raw("m2", "t", "2"), raw("m3", "t", "3")],
            next: Some(ProviderCursor("c2".into())),
            more: false,
        },
    ]);
    let report = repo.sync(me.id).await.unwrap();
    assert_eq!((report.stored, report.duplicates), (3, 1));
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 3);

    repo.sync(me.id).await.unwrap();
    let since = provider.since.lock().unwrap().clone();
    assert_eq!(
        since,
        vec![
            None,
            Some(ProviderCursor("c1".into())),
            Some(ProviderCursor("c2".into()))
        ],
        "the third fetch starts from the cursor stored by the second"
    );
}

#[tokio::test]
async fn a_cursor_survives_a_restart() {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let repo = repo_over(store.clone(), 50).await;
    let me = account(&repo, "me@example.org").await;
    let p = TestProvider::new(Encryption::Mls, &[]);
    p.batches.lock().unwrap().push_back(FetchBatch {
        messages: vec![],
        next: Some(ProviderCursor("c1".into())),
        more: false,
    });
    repo.register_provider(me.id, p);
    repo.sync(me.id).await.unwrap();

    drop(repo);
    let later = repo_over(store, 50).await;
    let p2 = TestProvider::new(Encryption::Mls, &[]);
    later.register_provider(me.id, p2.clone());
    later.sync(me.id).await.unwrap();
    assert_eq!(
        p2.since.lock().unwrap()[0],
        Some(ProviderCursor("c1".into()))
    );
}

#[tokio::test]
async fn sync_without_provider_says_so() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    assert!(matches!(repo.sync(me.id).await, Err(Error::Provider(_))));
}

#[tokio::test]
async fn a_reply_finds_its_parent_in_either_order() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut child = raw("child", "t", "answer");
    child.in_reply_to = Some(alias("parent"));
    let Ingested::Stored(child_id) = repo.ingest(me.id, child).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let waiting = repo.message(conv, child_id).await.unwrap();
    assert_eq!(waiting.summary.envelope.in_reply_to, None);
    assert_eq!(waiting.pending_reply, Some(alias("parent")));

    let Ingested::Stored(parent) = repo
        .ingest(me.id, raw("parent", "t", "question"))
        .await
        .unwrap()
    else {
        panic!();
    };
    let adopted = repo.message(conv, child_id).await.unwrap();
    assert_eq!(
        adopted.summary.envelope.in_reply_to,
        Some(parent.to_string())
    );
    assert_eq!(adopted.pending_reply, None);

    let mut late = raw("late", "t", "another");
    late.in_reply_to = Some(alias("parent"));
    let Ingested::Stored(late_id) = repo.ingest(me.id, late).await.unwrap() else {
        panic!();
    };
    let direct = repo.message(conv, late_id).await.unwrap();
    assert_eq!(
        direct.summary.envelope.in_reply_to,
        Some(parent.to_string())
    );
}
