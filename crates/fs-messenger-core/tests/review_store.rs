//! N14, file names, one handle per store, a kind that is given.

mod common;

use std::sync::Arc;

use common::{account, alias, conversation_of, raw, repo_over, setup, with_provider};
use fs_messenger_core::{
    DeliveryState, Encryption, Error, Ingest, Ingested, Kind, MessageAction, Messenger,
    MessengerRepo, RawAttachment, RawMessage, ToolCallState,
};
use fs_store::{ObjectId, Store, TursoStore};
use serde_json::json;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn n14_what_this_version_does_not_know_survives_a_change() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let id = stored(&repo, me.id, raw("uid-1", "t", "x")).await;
    let conv = conversation_of(&repo).await;
    let mut newer = store.get(id).await.unwrap().unwrap();
    newer.content["freesynergy.net:delivery"] = json!("queued");
    newer.content["freesynergy.net:encryption"] = json!("quantum");
    newer.content["futureField"] = json!({ "kept": true });
    newer.content["toolCalls"] = json!([{ "id": "c1", "state": "paused", "future": 1 }]);
    store.put(newer, "newer-version").await.unwrap();

    repo.act(conv, id, MessageAction::Flag(true)).await.unwrap();

    let c = store.get(id).await.unwrap().unwrap().content;
    assert_eq!(c["flagged"], json!(true));
    assert_eq!(c["freesynergy.net:delivery"], json!("queued"));
    assert_eq!(c["freesynergy.net:encryption"], json!("quantum"));
    assert_eq!(c["futureField"], json!({ "kept": true }));
    assert_eq!(c["toolCalls"][0]["state"], json!("paused"));
    assert_eq!(c["toolCalls"][0]["future"], json!(1));
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(
        m.summary.state,
        DeliveryState::Unknown,
        "read as not known, written as it was"
    );
    assert_eq!(m.tool_calls[0].state, ToolCallState::Unknown);
}

#[tokio::test]
async fn n14_a_real_change_wins_over_what_was_there_also_one_to_unknown() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    provider.receipt.lock().unwrap().encryption = Encryption::Unknown;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    assert!(d.send(None).await.is_err(), "a downgrade");
    let conv = conversation_of(&repo).await;
    let page = repo.messages(conv, None).await.unwrap();
    assert_eq!(
        page.items[0].encryption,
        Encryption::Unknown,
        "not the planned mls"
    );
}

#[tokio::test]
async fn attachment_names_lose_control_characters() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("uid-1", "t", "x");
    m.attachments.push(RawAttachment {
        name: "report\r\n\u{202e}.txt\0".into(),
        media_type: "text/\nplain".into(),
        data: vec![1],
    });
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    let a = repo.message(conv, id).await.unwrap().attachments.remove(0);
    assert_eq!(a.name, "report\u{202e}.txt");
    assert_eq!(a.media_type, "text/plain");
}

#[tokio::test]
async fn a_second_handle_on_the_same_store_is_refused_until_the_first_is_gone() {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let first = repo_over(store.clone(), 50).await;
    let second = first.clone();
    let config = fs_messenger_core::MessengerConfig::new(common::ME);
    let refused = MessengerRepo::open(store.clone(), config.clone()).await;
    assert!(matches!(refused, Err(Error::Conflict(_))));
    drop(first);
    assert!(
        MessengerRepo::open(store.clone(), config.clone())
            .await
            .is_err(),
        "a clone holds it"
    );
    drop(second);
    assert!(MessengerRepo::open(store, config).await.is_ok());
}

#[test]
fn a_kind_is_given_not_guessed() {
    let m = RawMessage::new(alias("a"), alias("c"), Kind::Chat, "s", "t");
    assert_eq!(m.kind, Kind::Chat);
}
