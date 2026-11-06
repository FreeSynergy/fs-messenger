//! What leaves: through the core's `Draft.send`, with real crypto; only the wire is replaced.

mod common;

use std::sync::Arc;

use common::person;
use fs_messenger_core::{AccountSetup, Encryption, Error, Messenger, MessengerConfig, MessengerRepo};
use fs_messenger_mail::mime::MailFields;
use fs_messenger_mail::openpgp::{decrypt, generate_key};
use fs_store::{Store, TursoStore};

async fn core_with(p: &common::Offline) -> (MessengerRepo, fs_store::ObjectId) {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let repo = MessengerRepo::open(store, MessengerConfig::new("https://local.invalid/me"))
        .await
        .unwrap();
    let account = repo
        .add_account(AccountSetup {
            address: p.address.clone(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    repo.register_provider(account.id, p.provider.clone());
    (repo, account.id)
}

#[tokio::test]
async fn bcc_is_on_the_wire_only_and_the_payload_carries_protected_headers() {
    let me = person("me@example.org").await;
    let bob = generate_key("bob@example.org").unwrap();
    let hidden = generate_key("hidden@example.org").unwrap();
    me.provider.import_peer_key("bob@example.org", &bob.public()).await.unwrap();
    me.provider.import_peer_key("hidden@example.org", &hidden.public()).await.unwrap();
    let (repo, account) = core_with(&me).await;

    let mut d = repo.compose(None).await.unwrap();
    d.set_account(account).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec!["hidden@example.org".into()])
        .await
        .unwrap();
    d.set_text("Greetings\nmore text".into(), "text/plain".into()).await.unwrap();
    let id = d.send(None).await.unwrap();

    let sent = me.wire.sent.lock().unwrap();
    let (envelope, raw) = &sent[0];
    assert_eq!(envelope.recipients, ["bob@example.org", "hidden@example.org"]);
    let text = String::from_utf8_lossy(raw);
    assert!(!text.contains("hidden@"), "bcc in no outer header");
    assert!(!text.contains("Greetings"), "no subject or text outside");
    assert!(text.contains(&format!("{id}@example.org")), "the id comes back as the sent copy's");

    let outer = MailFields::read(raw).unwrap();
    let at = raw.windows(10).position(|w| w == b"-----BEGIN").unwrap();
    let keys = [hidden];
    let (payload, _) = decrypt(&raw[at..], &keys, &[]).expect("a hidden recipient can read it");
    let inner = MailFields::read(&payload).unwrap();
    assert!(inner.header_protected, "RFC 9788");
    assert_eq!(inner.subject.as_deref(), Some("Greetings"));
    assert_eq!(outer.subject.as_deref(), Some("[...]"));
    assert!(!String::from_utf8_lossy(&payload).contains("hidden@"), "bcc not inside either");
    assert!(!inner.autocrypt.is_empty(), "the key offer is under the signature too");
    assert!(String::from_utf8_lossy(&payload).contains("HP-Outer: Subject: [...]"));
}

#[tokio::test]
async fn nothing_reaches_a_strategy_without_a_key_for_everyone() {
    let me = person("me@example.org").await;
    let (repo, account) = core_with(&me).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_account(account).await.unwrap();
    d.set_recipients(vec!["nokey@example.org".into()], vec![], vec![]).await.unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    let Err(Error::EncryptionRefused { encryption, missing_keys }) = d.send(None).await else {
        panic!("refused");
    };
    assert_eq!(encryption, Encryption::None);
    assert_eq!(missing_keys, ["nokey@example.org"]);
    assert!(me.wire.sent.lock().unwrap().is_empty());
}
