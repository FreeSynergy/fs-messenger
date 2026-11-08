//! Accepted ideas 9 and 10 (`requirements.md`) through the messenger: why a conversation is not
//! encrypted, and the whole conversation in an open format. Real repository, real store.

mod common;

use common::{account, conversation_of, raw_at, setup};
use fs_messenger_core::{Ingest, Messenger, Remedy, WhyNot};

#[tokio::test]
async fn the_whole_conversation_leaves_oldest_first_in_an_open_format() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let me = repo.accounts().await.unwrap()[0].id;
    for n in 0..5 {
        repo.ingest(me, raw_at(&format!("m{n}"), "t", n))
            .await
            .unwrap();
    }
    let conv = conversation_of(&repo).await;
    let out = repo.export_conversation(conv).await.unwrap();
    let lines: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 5);
    let times: Vec<&str> = lines
        .iter()
        .map(|l| l["published"].as_str().unwrap_or_default())
        .collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted, "oldest first");
    assert!(lines.iter().all(|l| l["type"] == "Note"));
}

#[tokio::test]
async fn a_missing_conversation_is_an_error_not_an_empty_file() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let got = repo.export_conversation(fs_objects::ObjectId::new()).await;
    assert!(got.is_err(), "{got:?}");
}

#[tokio::test]
async fn a_plain_conversation_says_why_and_what_to_do() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let me = repo.accounts().await.unwrap()[0].id;
    let mut plain = raw_at("m0", "t", 0);
    plain.encryption = fs_messenger_core::Encryption::None;
    repo.ingest(me, plain).await.unwrap();
    let conv = conversation_of(&repo).await;
    let got = repo.why_not_encrypted(conv).await.unwrap();
    // A value to translate, never decided by comparing words.
    let (why, remedy) = got.expect("a plain mail is not end-to-end encrypted");
    assert!(matches!(
        why,
        WhyNot::AccountCannot | WhyNot::NoKeyOfTheOtherSide | WhyNot::NotKnown
    ));
    assert!(matches!(
        remedy,
        Remedy::ChooseOtherChannel | Remedy::ExchangeKeys | Remedy::Wait | Remedy::TurnOn
    ));
}

#[tokio::test]
async fn an_encrypted_conversation_needs_no_explanation() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let me = repo.accounts().await.unwrap()[0].id;
    repo.ingest(me, raw_at("m0", "t", 0)).await.unwrap(); // the fixture is OpenPGP
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.why_not_encrypted(conv).await.unwrap(), None);
}
