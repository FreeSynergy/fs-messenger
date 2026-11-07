//! What the mail provider asked for: subject, the `References` chain, undecrypted and signature.

mod common;

use common::{account, conversation_of, message_id, raw, setup, with_provider};
use fs_messenger_core::{Encryption, Ingest, Ingested, Messenger, MessengerRepo, SignatureCheck};
use fs_objects::ObjectId;
use serde_json::{from_value, json};

async fn stored(repo: &MessengerRepo, me: ObjectId, m: fs_messenger_core::RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn a_subject_goes_out_names_the_message_and_survives_a_resume() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_subject("Lunch\0 on Friday".into()).await.unwrap();
    d.set_text("See you".into(), "text/plain".into())
        .await
        .unwrap();
    let saved = d.save().await.unwrap();

    let mut again = repo.resume_draft(saved).await.unwrap();
    let id = again.send(None).await.unwrap();
    assert_eq!(
        provider.sent.lock().unwrap()[0].subject(),
        "Lunch on Friday"
    );
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap();
    assert_eq!(shown.summary.envelope.name, "Lunch on Friday");
    assert_eq!(shown.body_text, "See you");
}

#[tokio::test]
async fn without_a_subject_the_outgoing_one_is_empty_and_the_name_comes_from_the_text() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("See you".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].subject(), "");
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap();
    assert_eq!(shown.summary.envelope.name, "See you");
}

#[tokio::test]
async fn a_reply_carries_the_parents_chain_and_a_sent_message_continues_it() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["ada@example.org"]).await;
    provider.receipt.lock().unwrap().aliases = vec![message_id("mine@x")];
    let mut parent = raw("uid-1", "t", "q");
    parent.links.push(message_id("parent@x"));
    parent.references = vec![message_id("root@x"), message_id("mid@x")];
    let parent = stored(&repo, me.id, parent).await;

    let mut d = repo.compose(Some(parent)).await.unwrap();
    d.set_text("a".into(), "text/plain".into()).await.unwrap();
    let mine = d.send(None).await.unwrap();
    let sent = provider.sent.lock().unwrap()[0].clone();
    assert_eq!(
        sent.references(),
        [message_id("root@x"), message_id("mid@x")]
    );
    assert!(sent.in_reply_to().contains(&message_id("parent@x")));

    let mut d = repo.compose(Some(mine)).await.unwrap();
    d.set_text("again".into(), "text/plain".into())
        .await
        .unwrap();
    d.send(None).await.unwrap();
    let next = provider.sent.lock().unwrap()[1].clone();
    assert_eq!(
        next.references(),
        [
            message_id("root@x"),
            message_id("mid@x"),
            message_id("parent@x")
        ]
    );
    assert!(next.in_reply_to().contains(&message_id("mine@x")));
}

#[tokio::test]
async fn a_new_message_has_no_chain_and_a_senders_chain_is_bounded() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.send(None).await.unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].references().len(), 0);

    let mut long = raw("uid-1", "t", "x");
    long.references = (0..500).map(|n| message_id(&format!("{n}@x"))).collect();
    long.references.push(message_id("bad|part@x"));
    let id = stored(&repo, me.id, long).await;
    let page = repo.conversations(None, false, None).await.unwrap();
    let conv = page
        .items
        .iter()
        .find(|c| c.name == "ada@example.org")
        .unwrap()
        .id;
    let kept = repo.message(conv, id).await.unwrap().references;
    assert_eq!(kept.len(), 64);
    assert_eq!(
        kept.last().unwrap(),
        &message_id("499@x"),
        "the newest stay"
    );
}

#[tokio::test]
async fn undecrypted_and_the_signature_check_are_typed_and_unset_is_not_verified() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let silent = stored(&repo, me.id, raw("uid-1", "t", "text")).await;
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, silent).await.unwrap();
    assert_eq!(m.summary.signature, SignatureCheck::NotVerified);
    assert!(!m.summary.undecrypted);

    let mut locked = raw("uid-2", "t", "");
    locked.undecrypted = true;
    let locked = stored(&repo, me.id, locked).await;
    let m = repo.message(conv, locked).await.unwrap();
    assert!(m.summary.undecrypted);
    assert_eq!(m.body_text, "", "the state is a field, not body text");
    assert_ne!(m.summary.envelope.name.len(), 0);
}

#[test]
fn a_value_this_version_does_not_know_is_not_verified() {
    let read: SignatureCheck = from_value(json!("quantumSigned")).unwrap();
    assert_eq!(read, SignatureCheck::NotVerified);
    assert_eq!(SignatureCheck::default(), SignatureCheck::NotVerified);
}

#[tokio::test]
async fn an_old_record_reads_as_not_verified() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let id = stored(&repo, me.id, raw("uid-1", "t", "text")).await;
    let conv = conversation_of(&repo).await;
    let mut old = store.get(id).await.unwrap().unwrap();
    old.content = json!({ "content": "old" });
    store.put(old, "old-version").await.unwrap();
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.signature, SignatureCheck::NotVerified);
    assert_eq!(m.references.len(), 0);
}
