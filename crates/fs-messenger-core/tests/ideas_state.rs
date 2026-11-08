//! Accepted ideas 4 and 1 through the messenger: read receipts only both ways, and a message held
//! for a while that can be sent at once, called back while held, and is released when due, once.

mod common;

use common::{setup, with_provider};
use fs_messenger_core::{
    COOL_DOWN_SECS, CoolDown, Encryption, Error, Messenger,
    ReceiptChoice::{Hidden, Shown},
};

#[tokio::test]
async fn receipts_are_hidden_until_chosen_and_need_both() {
    let (repo, _) = setup().await;
    assert_eq!(repo.receipt_choice().await.unwrap(), Hidden);
    assert!(!repo.show_receipt(Shown).await.unwrap());
    repo.set_receipt_choice(Shown).await.unwrap();
    assert_eq!(repo.receipt_choice().await.unwrap(), Shown);
    assert!(repo.show_receipt(Shown).await.unwrap());
    assert!(!repo.show_receipt(Hidden).await.unwrap());
    repo.set_receipt_choice(Hidden).await.unwrap();
    assert!(
        !repo.show_receipt(Shown).await.unwrap(),
        "one side hiding hides it for both"
    );
}

async fn saved_draft(repo: &fs_messenger_core::MessengerRepo, text: &str) -> fs_objects::ObjectId {
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text(text.into(), "text/plain".into()).await.unwrap();
    d.save().await.unwrap()
}

#[tokio::test]
async fn a_held_message_waits_goes_out_when_due_and_only_once() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let draft = saved_draft(&repo, "calm down first").await;
    let held = repo.hold(draft, 1_000, None).await.unwrap();
    assert_eq!(
        repo.held_stage(held).await.unwrap(),
        CoolDown::Held {
            release_at: 1_000 + COOL_DOWN_SECS
        }
    );

    assert_eq!(
        repo.release_due(1_000 + COOL_DOWN_SECS - 1).await.unwrap(),
        [] as [fs_objects::ObjectId; 0]
    );
    assert_eq!(provider.sent_count(), 0, "nothing before its time");
    assert_eq!(
        repo.release_due(1_000 + COOL_DOWN_SECS).await.unwrap(),
        [held]
    );
    assert_eq!(provider.sent_count(), 1);
    assert_eq!(repo.held_stage(held).await.unwrap(), CoolDown::SentUnread);
    assert!(
        repo.release_due(9_999_999).await.unwrap().is_empty(),
        "not twice"
    );
    assert_eq!(provider.sent_count(), 1);
}

#[tokio::test]
async fn send_now_sends_at_once_and_a_recall_works_only_while_held() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;

    let a = saved_draft(&repo, "a").await;
    let held_a = repo.hold(a, 0, Some(60)).await.unwrap();
    repo.send_held_now(held_a, 5).await.unwrap();
    assert_eq!(provider.sent_count(), 1);
    assert!(matches!(
        repo.send_held_now(held_a, 6).await,
        Err(Error::Conflict(_))
    ));
    assert!(
        matches!(repo.recall_held(held_a).await, Err(Error::Conflict(_))),
        "out is out"
    );

    let b = saved_draft(&repo, "b").await;
    let held_b = repo.hold(b, 0, Some(60)).await.unwrap();
    repo.recall_held(held_b).await.unwrap();
    assert_eq!(repo.held_stage(held_b).await.unwrap(), CoolDown::Recalled);
    assert_eq!(
        repo.release_due(10_000).await.unwrap(),
        [] as [fs_objects::ObjectId; 0]
    );
    assert_eq!(
        provider.sent_count(),
        1,
        "a recalled message never goes out"
    );
    // The draft is still there to edit.
    assert!(repo.resume_draft(b).await.is_ok());
}

#[tokio::test]
async fn only_a_saved_draft_can_be_held() {
    let (repo, _) = setup().await;
    let got = repo.hold(fs_objects::ObjectId::new(), 0, None).await;
    assert!(matches!(got, Err(Error::NotFound(_))), "{got:?}");
}

#[tokio::test]
async fn a_failed_send_keeps_the_message_held_for_the_next_try() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let draft = saved_draft(&repo, "x").await;
    let held = repo.hold(draft, 0, Some(10)).await.unwrap();
    *provider.fail_send.lock().unwrap() = true;
    assert!(repo.release_due(100).await.is_err());
    assert_eq!(provider.sent_count(), 0);
    assert!(matches!(
        repo.held_stage(held).await.unwrap(),
        CoolDown::Held { .. }
    ));
    *provider.fail_send.lock().unwrap() = false;
    assert_eq!(repo.release_due(101).await.unwrap(), [held]);
    assert_eq!(provider.sent_count(), 1);
}

#[tokio::test]
async fn one_message_that_cannot_go_does_not_hold_the_others_back() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let gone = saved_draft(&repo, "its draft will be deleted").await;
    let held_gone = repo.hold(gone, 0, Some(1)).await.unwrap();
    repo.resume_draft(gone)
        .await
        .unwrap()
        .discard()
        .await
        .unwrap();
    let fine = saved_draft(&repo, "fine").await;
    let held_fine = repo.hold(fine, 0, Some(1)).await.unwrap();
    let went = repo.release_due(10).await.unwrap();
    assert_eq!(went, [held_fine]);
    assert_eq!(provider.sent_count(), 1);
    assert!(
        matches!(
            repo.held_stage(held_gone).await.unwrap(),
            CoolDown::Held { .. }
        ),
        "tried again later"
    );
    // With nothing else to send, the failure is reported.
    assert!(repo.release_due(11).await.is_err());
}

#[tokio::test]
async fn a_send_that_came_back_as_a_downgrade_is_listed_as_unsure_and_never_sent_again() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().encryption = Encryption::TransportOnly;
    let draft = saved_draft(&repo, "weaker than planned").await;
    let held = repo.hold(draft, 0, Some(1)).await.unwrap();
    let none: Vec<fs_objects::ObjectId> = vec![];
    assert_eq!(repo.held_unsure().await.unwrap(), none);
    let got = repo.release_due(10).await;
    assert!(
        matches!(got, Ok(_) | Err(Error::EncryptionDowngraded { .. })),
        "{got:?}"
    );
    assert_eq!(repo.held_unsure().await.unwrap(), [held]);
    assert_eq!(repo.held_stage(held).await.unwrap(), CoolDown::SentUnread);
    assert!(repo.release_due(99).await.unwrap().is_empty(), "not again");
    assert_eq!(provider.sent_count(), 1);
}
