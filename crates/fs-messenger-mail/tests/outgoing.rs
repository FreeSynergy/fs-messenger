//! What leaves: through the core's `Draft.send`, with real crypto; only the wire is replaced.

mod common;

use common::{core_with, person};
use fs_messenger_core::{Encryption, Error, Messenger};
use fs_messenger_mail::mime::{MailFields, PayloadInput, build_outer, build_payload};
use fs_messenger_mail::openpgp::{decrypt, generate_key};

#[tokio::test]
async fn bcc_is_on_the_wire_only_and_the_payload_carries_protected_headers() {
    let me = person("me@example.org").await;
    let bob = generate_key("bob@example.org").unwrap();
    let hidden = generate_key("hidden@example.org").unwrap();
    me.provider
        .import_peer_key("bob@example.org", &bob.public())
        .await
        .unwrap();
    me.provider
        .import_peer_key("hidden@example.org", &hidden.public())
        .await
        .unwrap();
    let (repo, account) = core_with(&me).await;

    let mut d = repo.compose(None).await.unwrap();
    d.set_account(account).await.unwrap();
    d.set_recipients(
        vec!["bob@example.org".into()],
        vec![],
        vec!["hidden@example.org".into()],
    )
    .await
    .unwrap();
    d.set_subject("Greetings".into()).await.unwrap();
    d.set_text("first line\nmore text".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.send(None).await.unwrap();

    let sent = me.wire.sent.lock().unwrap();
    let (envelope, raw) = &sent[0];
    assert_eq!(
        envelope.recipients,
        ["bob@example.org", "hidden@example.org"]
    );
    let text = String::from_utf8_lossy(raw);
    assert!(!text.contains("hidden@"), "bcc in no outer header");
    assert!(!text.contains("Greetings"), "no subject or text outside");
    assert!(
        text.contains(&format!("{id}@example.org")),
        "the id comes back as the sent copy's"
    );

    let outer = MailFields::read(raw).unwrap();
    let at = raw.windows(10).position(|w| w == b"-----BEGIN").unwrap();
    let keys = [hidden];
    let (payload, _) =
        decrypt(&raw[at..], &keys, &[], 1 << 20).expect("a hidden recipient can read it");
    let inner = MailFields::read(&payload).unwrap();
    assert!(inner.header_protected, "RFC 9788");
    assert_eq!(inner.subject.as_deref(), Some("Greetings"));
    assert_eq!(outer.subject.as_deref(), Some("[...]"));
    assert!(
        !String::from_utf8_lossy(&payload).contains("hidden@"),
        "bcc not inside either"
    );
    assert!(
        !inner.autocrypt.is_empty(),
        "the key offer is under the signature too"
    );
    assert!(String::from_utf8_lossy(&payload).contains("HP-Outer: Subject: [...]"));
}

#[tokio::test]
async fn nothing_reaches_a_strategy_without_a_key_for_everyone() {
    let me = person("me@example.org").await;
    let (repo, account) = core_with(&me).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_account(account).await.unwrap();
    d.set_recipients(vec!["nokey@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    let Err(Error::EncryptionRefused {
        encryption,
        missing_keys,
    }) = d.send(None).await
    else {
        panic!("refused");
    };
    assert_eq!(encryption, Encryption::None);
    assert_eq!(missing_keys, ["nokey@example.org"]);
    assert!(me.wire.sent.lock().unwrap().is_empty());
}

/// RFC 9788: every field that went outside has its `HP-Outer` line, saying what is really out there.
#[tokio::test]
async fn hp_outer_names_all_outer_fields() {
    let from = "me@example.org";
    let to = ["bob@example.org".to_string()];
    let chain = ["root@e".to_string(), "parent@e".to_string()];
    let input = PayloadInput {
        from,
        to: &to,
        cc: &[],
        subject: "secret",
        message_id: "m@e",
        in_reply_to: Some("parent@e"),
        references: &chain,
        date: 1_793_000_000,
        text: "t",
        media_type: "text/plain",
        attachments: &[],
        autocrypt: "",
    };
    let payload = String::from_utf8(build_payload(&input).unwrap()).unwrap();
    let outer = String::from_utf8(build_outer(&input, "armor").unwrap()).unwrap();
    let date = outer
        .lines()
        .find_map(|l| l.strip_prefix("Date: "))
        .unwrap();
    for line in [
        format!("HP-Outer: Date: {date}"),
        format!("HP-Outer: From: {from}"),
        "HP-Outer: Subject: [...]".to_string(),
        "HP-Outer: Message-ID: <m@e>".to_string(),
        "HP-Outer: To: bob@example.org".to_string(),
        "HP-Outer: In-Reply-To: <parent@e>".to_string(),
        "HP-Outer: References: <root@e> <parent@e>".to_string(),
    ] {
        assert!(payload.contains(&line), "missing {line} in {payload}");
    }
}

/// With no subject there is none inside, and the placeholder stays outside. Nothing is made up.
#[tokio::test]
async fn no_subject_is_no_subject() {
    let me = person("me@example.org").await;
    let bob = generate_key("bob@example.org").unwrap();
    me.provider
        .import_peer_key("bob@example.org", &bob.public())
        .await
        .unwrap();
    let (repo, account) = core_with(&me).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_account(account).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("only text".into(), "text/plain".into())
        .await
        .unwrap();
    d.send(None).await.unwrap();
    let sent = me.wire.sent.lock().unwrap();
    let raw = &sent[0].1;
    let at = raw.windows(10).position(|w| w == b"-----BEGIN").unwrap();
    let (payload, _) = decrypt(&raw[at..], &[bob], &[], 1 << 20).unwrap();
    assert_eq!(MailFields::read(&payload).unwrap().subject, None);
    assert_eq!(
        MailFields::read(raw).unwrap().subject.as_deref(),
        Some("[...]")
    );
}
