//! Mark read up to, next unread, unread counts.

mod common;

use common::{account, conversation_of, raw_at, setup};
use fs_messenger_core::{Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo};
use fs_objects::ObjectId;

async fn three(repo: &MessengerRepo, thread: &str, base: u32) -> Vec<ObjectId> {
    let me = repo.accounts().await.unwrap()[0].id;
    let mut ids = vec![];
    for n in 0..3 {
        let name = format!("{thread}{n}");
        let got = repo
            .ingest(me, raw_at(&name, thread, base + n))
            .await
            .unwrap();
        let Ingested::Stored(id) = got else { panic!() };
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn read_up_to_x_marks_x_and_everything_older_and_nothing_else() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let ids = three(&repo, "t", 0).await;
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 3);

    repo.mark_read(conv, ids[1]).await.unwrap();

    assert!(!repo.message(conv, ids[0]).await.unwrap().summary.unread);
    assert!(!repo.message(conv, ids[1]).await.unwrap().summary.unread);
    assert!(repo.message(conv, ids[2]).await.unwrap().summary.unread);
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 1);
}

#[tokio::test]
async fn a_message_of_another_conversation_is_not_found_here() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let a = three(&repo, "a", 0).await;
    three(&repo, "b", 10).await;
    let list = repo.conversations(None, false, None).await.unwrap().items;
    let other = list
        .iter()
        .find(|c| c.last_message.as_ref().unwrap().preview == "b2")
        .unwrap()
        .id;
    assert!(matches!(
        repo.mark_read(other, a[0]).await,
        Err(Error::NotFound(_))
    ));
    assert_eq!(repo.open(other).await.unwrap().unread_count, 3);
}

#[tokio::test]
async fn next_unread_walks_all_conversations_oldest_first_and_goes_on_after_a_delete() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let a = three(&repo, "a", 0).await;
    let b = three(&repo, "b", 10).await;

    let first = repo.next_unread(None).await.unwrap().unwrap();
    assert_eq!(first.envelope.id, a[0]);
    let next = repo.next_unread(Some(a[2])).await.unwrap().unwrap();
    assert_eq!(next.envelope.id, b[0]);

    let conv_of_b: ObjectId = next.envelope.context.as_deref().unwrap().parse().unwrap();
    repo.act(conv_of_b, b[0], MessageAction::Delete)
        .await
        .unwrap();
    let after_delete = repo.next_unread(Some(b[0])).await.unwrap().unwrap();
    assert_eq!(after_delete.envelope.id, b[1]);

    repo.mark_read(conv_of_b, b[2]).await.unwrap();
    assert!(repo.next_unread(Some(a[2])).await.unwrap().is_none());
}

#[tokio::test]
async fn marking_unread_counts_again() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let ids = three(&repo, "t", 0).await;
    let conv = conversation_of(&repo).await;
    repo.mark_read(conv, ids[2]).await.unwrap();
    repo.act(conv, ids[0], MessageAction::Unread(true))
        .await
        .unwrap();
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 1);
    assert_eq!(
        repo.next_unread(None).await.unwrap().unwrap().envelope.id,
        ids[0]
    );
    repo.act(conv, ids[0], MessageAction::Unread(false))
        .await
        .unwrap();
    assert_eq!(repo.open(conv).await.unwrap().unread_count, 0);
}
