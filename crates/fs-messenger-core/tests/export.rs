//! Export of a conversation: AS2 objects, one per line, nothing lost that a person wrote.

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::export::{AS2_CONTEXT, conversation_jsonl};
use fs_messenger_core::{Ingest, Ingested, Messenger};

#[tokio::test]
async fn a_conversation_leaves_as_one_activity_streams_object_per_line() {
    let (repo, _) = setup().await;
    account(&repo, "me@example.org").await;
    let me = repo.accounts().await.unwrap()[0].id;
    let mut ids = vec![];
    for n in 0..2 {
        let got = repo
            .ingest(me, raw(&format!("m{n}"), "t", &format!("text {n}")))
            .await
            .unwrap();
        let Ingested::Stored(id) = got else { panic!() };
        ids.push(id);
    }
    let conv = conversation_of(&repo).await;
    let mut all = vec![];
    for id in ids {
        all.push(repo.message(conv, id).await.unwrap());
    }
    let out = conversation_jsonl(&all).unwrap();
    let lines: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    for (n, line) in lines.iter().enumerate() {
        assert_eq!(line["@context"], AS2_CONTEXT);
        assert_eq!(line["type"], "Note");
        assert!(
            line["content"]
                .as_str()
                .unwrap()
                .contains(&format!("text {n}"))
        );
        assert!(line["mediaType"].is_string());
    }
    assert!(out.ends_with('\n'));
}

#[test]
fn nothing_gives_nothing() {
    assert_eq!(conversation_jsonl(&[]).unwrap(), "");
}
