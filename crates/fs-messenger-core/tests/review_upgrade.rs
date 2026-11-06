//! N5, N15: an undecrypted message, and the readable version that replaces it.

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::{
    Encryption, Error, Ingest, Ingested, MessageAction, Messenger, MessengerRepo, RawAttachment,
    RawMessage, SearchFilter, SignatureCheck,
};
use fs_store::ObjectId;

async fn stored(repo: &MessengerRepo, me: ObjectId, m: RawMessage) -> ObjectId {
    let Ingested::Stored(id) = repo.ingest(me, m).await.unwrap() else {
        panic!("expected a new message");
    };
    id
}

#[tokio::test]
async fn n5_undecrypted_has_no_ciphertext_in_preview_name_or_search() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m = raw("uid-1", "t", "-----BEGIN PGP MESSAGE----- wcBMA");
    m.undecrypted = true;
    let id = stored(&repo, me.id, m).await;
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap().summary;
    assert!(shown.undecrypted);
    assert_eq!(shown.preview.len(), 0);
    assert!(!shown.envelope.name.contains("PGP"));
    let filter = SearchFilter::default();
    assert_eq!(
        repo.search("wcBMA", &filter, None)
            .await
            .unwrap()
            .items
            .len(),
        0
    );
    let forward = MessageAction::Forward(vec!["b@x".into()]);
    assert!(matches!(
        repo.act(conv, id, forward).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn n5_a_readable_version_replaces_an_undecrypted_one_under_the_same_id() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut locked = raw("uid-1", "t", "ciphertext");
    locked.undecrypted = true;
    locked.encryption = Encryption::None;
    let id = stored(&repo, me.id, locked).await;
    let conv = conversation_of(&repo).await;
    repo.act(conv, id, MessageAction::Flag(true)).await.unwrap();

    let mut readable = raw("uid-1", "t", "the real words");
    readable.signature = SignatureCheck::Valid;
    readable.encryption = Encryption::Openpgp;
    readable.subject = Some("Inner subject".into());
    readable.attachments.push(RawAttachment {
        name: "a".into(),
        media_type: "text/plain".into(),
        data: vec![1],
    });
    assert_eq!(
        repo.ingest(me.id, readable).await.unwrap(),
        Ingested::Duplicate(id)
    );

    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.body_text, "the real words");
    assert!(!m.summary.undecrypted);
    assert_eq!(m.summary.preview, "the real words");
    assert_eq!(m.summary.envelope.name, "Inner subject");
    assert_eq!(m.summary.signature, SignatureCheck::Valid);
    assert_eq!(m.summary.encryption, Encryption::Openpgp);
    assert_eq!(m.attachments.len(), 1);
    assert!(m.summary.flagged, "what the person did stays");
    let filter = SearchFilter::default();
    assert_eq!(
        repo.search("real", &filter, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        repo.open(conv).await.unwrap().encryption,
        Encryption::Openpgp
    );

    let mut again = raw("uid-1", "t", "another");
    again.undecrypted = false;
    repo.ingest(me.id, again).await.unwrap();
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(
        m.body_text, "the real words",
        "only an undecrypted one is replaced"
    );
}

#[tokio::test]
async fn n15_a_signature_needs_a_sender_and_readable_content() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut good = raw("uid-1", "t", "x");
    good.signature = SignatureCheck::Valid;
    let good = stored(&repo, me.id, good).await;
    let mut nobody = raw("uid-2", "t", "x");
    nobody.sender = String::new();
    nobody.signature = SignatureCheck::Valid;
    let nobody = stored(&repo, me.id, nobody).await;
    let mut locked = raw("uid-3", "t", "x");
    locked.undecrypted = true;
    locked.signature = SignatureCheck::NotSigned;
    let locked = stored(&repo, me.id, locked).await;
    let conv = conversation_of(&repo).await;
    let check = |id| {
        let repo = repo.clone();
        async move { repo.message(conv, id).await.unwrap().summary.signature }
    };
    assert_eq!(check(good).await, SignatureCheck::Valid);
    assert_eq!(check(nobody).await, SignatureCheck::NotVerified);
    assert_eq!(check(locked).await, SignatureCheck::NotVerified);
}
