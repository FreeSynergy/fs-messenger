//! M3: control characters reach neither a mail header nor a reply's recipients.

mod common;

use common::{conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    Encryption, Error, Ingest, Ingested, Messenger, MessengerRepo, RawMessage,
};
use fs_store::ObjectId;
use serde_json::json;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn m3_crlf_in_a_sender_never_reaches_a_replys_recipients() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@x.orgBcc: e@x"]).await;
    let mut m = raw("uid-1", "t", "q");
    m.sender = "ada@x.org\r\nBcc: e@x".into();
    m.subject = Some("a\r\nb".into());
    m.to = vec!["c\r\nd".into()];
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap().summary;
    assert_eq!(
        shown.envelope.attributed_to.as_deref(),
        Some("ada@x.orgBcc: e@x")
    );
    assert_eq!(shown.envelope.to, ["cd"]);
    let mut d = repo.compose(Some(id)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].to(), ["ada@x.orgBcc: e@x"]);
}

#[tokio::test]
async fn m3_set_recipients_refuses_and_set_subject_strips_control_characters() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["b@x"]).await;
    let mut d = repo.compose(None).await.unwrap();
    for bad in [
        (vec!["b@x\r\nBcc: e@x".to_string()], vec![], vec![]),
        (vec![], vec!["c\n".to_string()], vec![]),
        (vec![], vec![], vec!["d\t".to_string()]),
    ] {
        let got = d.set_recipients(bad.0, bad.1, bad.2).await;
        assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
    }
    d.set_recipients(vec!["b@x".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_subject("Hi\r\nBcc: e@x".into()).await.unwrap();
    d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].subject(), "HiBcc: e@x");
}

#[tokio::test]
async fn m3_what_was_stored_before_the_rule_is_refused_on_the_way_out() {
    let (repo, store) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["a\r\nb"]).await;
    let id = stored(&repo, me.id, raw("uid-1", "t", "q")).await;
    let mut old = store.get(id).await.unwrap().unwrap();
    old.envelope.attributed_to = Some("a\r\nb".into());
    store.put(old, "old-version").await.unwrap();
    let mut d = repo.compose(Some(id)).await.unwrap();
    let got = d.send(None).await;
    assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
    assert_eq!(provider.sent_count(), 0);
}

#[tokio::test]
async fn m3_the_subject_of_an_older_draft_is_clean_on_the_way_out() {
    let (repo, store) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["b@x"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["b@x".into()], vec![], vec![])
        .await
        .unwrap();
    let id = d.save().await.unwrap();
    let mut old = store.get(id).await.unwrap().unwrap();
    old.content["subject"] = json!("Hi\r\nBcc: e@x");
    store.put(old, "old-version").await.unwrap();

    let mut again = repo.resume_draft(id).await.unwrap();
    again.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].subject(), "HiBcc: e@x");
}
