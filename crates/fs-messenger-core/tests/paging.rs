//! Cursor paging and ordering, with real store queries.

mod common;

use std::sync::Arc;

use common::{account, raw_at, repo_over};
use fs_messenger_core::{Cursor, Ingest, Messenger};
use fs_store::{Store, TursoStore};

async fn small_pages(size: usize) -> fs_messenger_core::MessengerRepo {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    repo_over(store, size).await
}

#[tokio::test]
async fn messages_come_newest_first_by_publication_not_by_arrival() {
    let repo = small_pages(50).await;
    let me = account(&repo, "me@example.org").await;
    for (id, minute) in [("b", 2), ("c", 3), ("a", 1)] {
        repo.ingest(me.id, raw_at(id, "t", minute)).await.unwrap();
    }
    let conv = common::conversation_of(&repo).await;
    let page = repo.messages(conv, None).await.unwrap();
    let order: Vec<_> = page.items.iter().map(|m| m.preview.as_str()).collect();
    assert_eq!(order, ["c", "b", "a"]);
    assert_eq!(page.next, None);
}

#[tokio::test]
async fn pages_cover_everything_once_and_a_new_message_does_not_shift_them() {
    let repo = small_pages(2).await;
    let me = account(&repo, "me@example.org").await;
    for minute in 1..=5 {
        repo.ingest(me.id, raw_at(&format!("m{minute}"), "t", minute))
            .await
            .unwrap();
    }
    let conv = common::conversation_of(&repo).await;
    let first = repo.messages(conv, None).await.unwrap();
    assert_eq!(first.items.len(), 2);
    let cursor = first.next.clone().expect("more");

    repo.ingest(me.id, raw_at("newest", "t", 30)).await.unwrap();

    let again = Cursor::from_opaque(cursor.as_str());
    let second = repo.messages(conv, Some(&again)).await.unwrap();
    let third = repo.messages(conv, second.next.as_ref()).await.unwrap();
    let seen: Vec<_> = [first.items, second.items, third.items]
        .concat()
        .iter()
        .map(|m| m.preview.clone())
        .collect();
    assert_eq!(seen, ["m5", "m4", "m3", "m2", "m1"]);
    assert_eq!(third.next, None);
}

#[tokio::test]
async fn messages_at_the_same_moment_are_neither_lost_nor_repeated() {
    let repo = small_pages(2).await;
    let me = account(&repo, "me@example.org").await;
    for n in 0..5 {
        repo.ingest(me.id, raw_at(&format!("m{n}"), "t", 7))
            .await
            .unwrap();
    }
    let conv = common::conversation_of(&repo).await;
    let mut seen = vec![];
    let mut cursor = None;
    loop {
        let page = repo.messages(conv, cursor.as_ref()).await.unwrap();
        seen.extend(page.items.iter().map(|m| m.preview.clone()));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        seen,
        ["m4", "m3", "m2", "m1", "m0"],
        "the last arrived on top"
    );
}

#[tokio::test]
async fn conversations_are_newest_first_and_pinned_on_top_with_paging() {
    let repo = small_pages(2).await;
    let me = account(&repo, "me@example.org").await;
    for (thread, minute) in [("old", 1), ("mid", 2), ("new", 3)] {
        let mut m = raw_at(thread, thread, minute);
        m.conversation_name = Some(thread.into());
        repo.ingest(me.id, m).await.unwrap();
    }
    let names = |items: &[fs_messenger_core::ConversationSummary]| {
        items.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    };
    let first = repo.conversations(None, false, None).await.unwrap();
    assert_eq!(names(&first.items), ["new", "mid"]);
    let second = repo
        .conversations(None, false, first.next.as_ref())
        .await
        .unwrap();
    assert_eq!(names(&second.items), ["old"]);
    assert_eq!(second.next, None);

    let old = second.items[0].id;
    repo.pin(old, true).await.unwrap();
    let pinned = repo.conversations(None, false, None).await.unwrap();
    assert_eq!(names(&pinned.items), ["old", "new"]);
    assert!(pinned.items[0].pinned);

    repo.pin(old, false).await.unwrap();
    let mut m = raw_at("later", "old", 9);
    m.conversation_name = None;
    repo.ingest(me.id, m).await.unwrap();
    let moved = repo.conversations(None, false, None).await.unwrap();
    assert_eq!(names(&moved.items)[0], "old", "a new message moves it up");
    assert_eq!(
        moved.items[0].last_message.as_ref().unwrap().preview,
        "later"
    );
}

#[tokio::test]
async fn folder_and_unread_filters_narrow_the_list() {
    let repo = small_pages(50).await;
    let me = account(&repo, "me@example.org").await;
    for (thread, minute) in [("a", 1), ("b", 2)] {
        let mut m = raw_at(thread, thread, minute);
        m.conversation_name = Some(thread.into());
        repo.ingest(me.id, m).await.unwrap();
    }
    let all = repo.conversations(None, false, None).await.unwrap().items;
    let (b, a) = (all[0].clone(), all[1].clone());

    repo.move_to(a.id, "work").await.unwrap();
    let work = repo
        .conversations(Some("work"), false, None)
        .await
        .unwrap()
        .items;
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].folders, ["work"]);
    repo.move_to(a.id, "home").await.unwrap();
    assert_eq!(
        repo.conversations(Some("work"), false, None)
            .await
            .unwrap()
            .items
            .len(),
        0
    );
    repo.move_to(a.id, "").await.unwrap();
    assert_eq!(repo.open(a.id).await.unwrap().folders.len(), 0);

    let id = b.last_message.unwrap().envelope.id;
    repo.mark_read(b.id, id).await.unwrap();
    let unread = repo.conversations(None, true, None).await.unwrap().items;
    assert_eq!(unread.len(), 1);
    assert_eq!(unread[0].id, a.id);
}
