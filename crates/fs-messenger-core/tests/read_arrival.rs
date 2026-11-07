//! C7, C13, C14: the read mark runs over local arrival, display over the sender's date.

mod common;

use chrono::{TimeZone, Utc};
use common::{account, at, conversation_of, raw, raw_at, setup};
use fs_messenger_core::{Ingest, Ingested, Messenger, MessengerRepo, RawMessage};
use fs_objects::ObjectId;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

fn dated(id: &str, year: i32) -> RawMessage {
    let mut m = raw(id, "t", id);
    m.published = Some(Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).unwrap());
    m
}

async fn unread(repo: &MessengerRepo, conv: ObjectId, id: ObjectId) -> bool {
    repo.message(conv, id).await.unwrap().summary.unread
}

#[tokio::test]
async fn c13_a_mail_dated_1970_arriving_after_the_mark_stays_unread() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let first = stored(&repo, me.id, raw_at("first", "t", 1)).await;
    let conv = conversation_of(&repo).await;
    repo.mark_read(conv, first).await.unwrap();
    let second = stored(&repo, me.id, raw_at("second", "t", 2)).await;

    let old = stored(&repo, me.id, dated("old", 1970)).await;

    repo.mark_read(conv, second).await.unwrap();
    assert!(unread(&repo, conv, old).await, "it arrived after that mark");
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 1);
    let next = repo.next_unread(None).await.unwrap().unwrap();
    assert_eq!(next.envelope.id, old);
}

#[tokio::test]
async fn c13_a_mail_dated_2999_cannot_make_what_arrived_after_it_read() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let future = stored(&repo, me.id, dated("future", 2999)).await;
    let later = stored(&repo, me.id, raw_at("later", "t", 1)).await;
    let last = stored(&repo, me.id, raw_at("last", "t", 2)).await;
    let conv = conversation_of(&repo).await;

    repo.mark_read(conv, future).await.unwrap();

    assert!(!unread(&repo, conv, future).await);
    assert!(unread(&repo, conv, later).await);
    assert!(unread(&repo, conv, last).await);
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 2);
}

#[tokio::test]
async fn c13_read_up_to_the_top_covers_what_arrived_before_it_not_after() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    // Shown on top, but arrived first.
    let newest_date = stored(&repo, me.id, raw_at("top", "t", 9)).await;
    let conv = conversation_of(&repo).await;
    // Arrived later, with an older date, so it is shown below.
    let below = stored(&repo, me.id, raw_at("below", "t", 1)).await;
    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(
        page.items[0].envelope.id, newest_date,
        "display goes by date"
    );

    repo.mark_read(conv, newest_date).await.unwrap();
    assert!(
        unread(&repo, conv, below).await,
        "it arrived after the top one"
    );

    repo.mark_read(conv, below).await.unwrap();
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 0);
}

#[tokio::test]
async fn c13_next_unread_follows_arrival_not_dates() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let a = stored(&repo, me.id, dated("a", 2999)).await;
    let b = stored(&repo, me.id, dated("b", 1970)).await;
    let c = stored(&repo, me.id, raw_at("c", "t", 1)).await;
    assert_eq!(
        repo.next_unread(None).await.unwrap().unwrap().envelope.id,
        a
    );
    assert_eq!(
        repo.next_unread(Some(a))
            .await
            .unwrap()
            .unwrap()
            .envelope
            .id,
        b
    );
    assert_eq!(
        repo.next_unread(Some(b))
            .await
            .unwrap()
            .unwrap()
            .envelope
            .id,
        c
    );
}

#[tokio::test]
async fn c13_the_sequence_goes_on_after_a_restart() {
    let store: std::sync::Arc<dyn fs_objects::Store> =
        std::sync::Arc::new(fs_objects::TursoStore::open_in_memory().await.unwrap());
    let repo = common::repo_over(store.clone(), 50).await;
    let me = account(&repo, "me@example.org").await;
    let before = stored(&repo, me.id, raw_at("before", "t", 1)).await;
    let conv = conversation_of(&repo).await;
    let first = repo.message(conv, before).await.unwrap().summary.arrival;

    drop(repo);
    let again = common::repo_over(store, 50).await;
    let after = stored(&again, me.id, raw_at("after", "t", 2)).await;
    assert!(again.message(conv, after).await.unwrap().summary.arrival > first);
    again.mark_read(conv, before).await.unwrap();
    assert!(unread(&again, conv, after).await);
}

#[tokio::test]
async fn c7_read_up_to_the_top_clears_everything_on_equal_dates() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    for n in 0..8 {
        repo.ingest(me.id, raw_at(&format!("m{n}"), "t", 5))
            .await
            .unwrap();
    }
    let conv = conversation_of(&repo).await;
    let top = repo.messages(conv, None).await.unwrap().items[0]
        .envelope
        .id;
    repo.mark_read(conv, top).await.unwrap();
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 0);
    assert!(repo.next_unread(None).await.unwrap().is_none());
}

#[tokio::test]
async fn c14_a_future_date_shows_but_does_not_hold_the_top() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let future = stored(&repo, me.id, dated("future", 2999)).await;
    let conv = conversation_of(&repo).await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let mut now = raw("now", "t", "now");
    now.published = None;
    let current = stored(&repo, me.id, now).await;

    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(page.items[0].envelope.id, current);
    let m = repo.message(conv, future).await.unwrap();
    assert_eq!(
        m.summary.envelope.published.format("%Y").to_string(),
        "2999"
    );
    let listed = repo.open(conv).await.unwrap().last_message.unwrap();
    assert_eq!(listed.envelope.id, current);
    assert!(at(0) < m.sort_time());
}
