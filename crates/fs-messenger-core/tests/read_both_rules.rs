//! N2: a message is swept only if it is at or before X in arrival AND at or below X on screen.

mod common;

use chrono::{TimeZone, Utc};
use common::{account, conversation_of, raw, raw_at, setup};
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
async fn n2_reading_a_backdated_mail_leaves_newer_unseen_mail_unread() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let newer = stored(&repo, me.id, raw_at("newer", "t", 5)).await;
    let conv = conversation_of(&repo).await;
    let backdated = stored(&repo, me.id, dated("old", 1970)).await;

    repo.mark_read(conv, backdated).await.unwrap();

    assert!(!unread(&repo, conv, backdated).await);
    assert!(
        unread(&repo, conv, newer).await,
        "it is above X on screen, and unseen"
    );
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 1);
}

#[tokio::test]
async fn n2_a_mail_dated_1970_after_the_mark_is_not_swept_by_a_later_one() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let first = stored(&repo, me.id, raw_at("first", "t", 1)).await;
    let conv = conversation_of(&repo).await;
    let backdated = stored(&repo, me.id, dated("old", 1970)).await;
    repo.mark_read(conv, first).await.unwrap();
    assert!(
        unread(&repo, conv, backdated).await,
        "below X on screen, but arrived after it"
    );
}

#[tokio::test]
async fn n2_both_rules_together_sweep_what_really_is_older() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let older = stored(&repo, me.id, raw_at("older", "t", 1)).await;
    let conv = conversation_of(&repo).await;
    let newer = stored(&repo, me.id, raw_at("newer", "t", 9)).await;
    let x = stored(&repo, me.id, raw_at("x", "t", 5)).await;
    let late = stored(&repo, me.id, raw_at("late", "t", 2)).await;

    repo.mark_read(conv, x).await.unwrap();

    assert!(!unread(&repo, conv, older).await, "before in both orders");
    assert!(!unread(&repo, conv, x).await);
    assert!(unread(&repo, conv, newer).await, "above X on screen");
    assert!(unread(&repo, conv, late).await, "arrived after X");
    assert_eq!(
        repo.next_unread(None).await.unwrap().unwrap().envelope.id,
        newer
    );
}

#[tokio::test]
async fn n2_the_summary_carries_what_the_contract_mirrors() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    stored(&repo, me.id, raw_at("a", "t", 1)).await;
    let conv = conversation_of(&repo).await;
    stored(&repo, me.id, raw_at("b", "t", 2)).await;
    let page = repo.messages(conv, None).await.unwrap();
    assert!(page.items[0].arrival > page.items[1].arrival);
    assert!(!page.items[0].undecrypted);
    assert_eq!(
        page.items[0].signature,
        fs_messenger_core::SignatureCheck::NotVerified
    );
}
