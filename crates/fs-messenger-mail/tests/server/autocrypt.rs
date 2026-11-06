//! A forged Autocrypt header must not replace a known key; a proven rotation does.

use fs_messenger_core::Encryption;
use fs_messenger_mail::autocrypt::format_header;
use fs_messenger_mail::keys::KeySource;
use fs_messenger_mail::mime::{PayloadInput, build_outer, build_payload};
use fs_messenger_mail::openpgp::{
    EncryptionTargets, generate_key, sign_and_encrypt, unix_now, usable_encryption_key,
};

use crate::common::{
    Fetch, Person, Submit, Who, message_id_of, raw_by_message_id, serial, smtp_raw, token,
};

fn forged_plain(t: &str, header: &str) -> String {
    format!(
        "From: {}\r\nTo: {}\r\nSubject: Forged\r\nMessage-ID: <{t}-forged@msgtest.test>\r\n\
         Autocrypt: {header}\r\nContent-Type: text/plain\r\n\r\nForged-{t}\r\n",
        Who::Alice.address(),
        Who::Bob.address(),
    )
}

/// Sent by bob's own login with alice's `From`, as a stranger would claim to be alice.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_forged_header_does_not_replace_a_known_key() {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Imap, Submit::Smtp).await;
    bob.knows(&alice).await;
    alice.knows(&bob).await;
    let genuine = alice.provider.ensure_own_key().await.unwrap().fingerprint();
    let evil = generate_key(&Who::Alice.address()).unwrap().public();
    let header = format_header(&Who::Alice.address(), &evil).unwrap();
    let t = token();

    // 1. A plain mail with the attacker's key.
    smtp_raw(Who::Bob, Who::Bob, forged_plain(&t, &header).as_bytes()).await;
    let got = bob.wait_for(&format!("Forged-{t}")).await;
    assert_eq!(got.summary.encryption, Encryption::None);
    let held = bob
        .provider
        .peer(&Who::Alice.address())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.fingerprint, genuine, "the known key stays");
    assert_eq!(
        held.candidate.as_ref().map(|c| c.fingerprint.clone()),
        Some(evil.fingerprint()),
        "and the offer is visible"
    );

    // 2. Worse: alice's genuine signed mail, with the attacker's header put on the outside.
    let text = format!("Genuine-{t}");
    alice
        .send(None, &[Who::Bob.address()], &[], &text, &[])
        .await
        .expect("send");
    let real = bob.wait_for(&text).await;
    let id = message_id_of(&real);
    let bytes = raw_by_message_id(Who::Bob, &id).await.unwrap();
    let wrapped = String::from_utf8(bytes).unwrap();
    let start = wrapped.find("From: ").expect("a From header");
    let body = wrapped[start..]
        .replace(id.as_str(), &format!("{t}-wrapped@msgtest.test"))
        .replace("Autocrypt: addr=", "X-Old-Autocrypt: addr=");
    let forged = format!("Autocrypt: {header}\r\n{body}");
    smtp_raw(Who::Bob, Who::Bob, forged.as_bytes()).await;
    let probe = format!("{t}-wrapped@msgtest.test");
    for _ in 0..60 {
        if raw_by_message_id(Who::Bob, &probe).await.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    fs_messenger_core::Ingest::sync(&bob.repo, bob.account)
        .await
        .expect("sync");
    let held = bob
        .provider
        .peer(&Who::Alice.address())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        held.fingerprint, genuine,
        "a valid signature does not cover an outer header"
    );

    // 3. What bob sends still reaches alice, under the genuine key.
    let answer = format!("Still-{t}");
    bob.send(None, &[Who::Alice.address()], &[], &answer, &[])
        .await
        .expect("send");
    let opened = alice.wait_for(&answer).await;
    assert_eq!(
        opened.summary.encryption,
        Encryption::Openpgp,
        "alice can read it"
    );
}

/// The new key inside the payload, signed with the old key: this is a rotation.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_rotation_signed_by_the_known_key_is_taken() {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Jmap, Submit::Smtp).await;
    bob.knows(&alice).await;
    let old = alice.provider.ensure_own_key().await.unwrap();
    let new = generate_key(&Who::Alice.address()).unwrap().public();
    let bob_public = bob.provider.ensure_own_key().await.unwrap().public();
    let t = token();
    let (from, to) = (Who::Alice.address(), vec![Who::Bob.address()]);
    let (body, id) = (format!("Rotate-{t}"), format!("{t}-rotate@msgtest.test"));
    let autocrypt = format_header(&from, &new).unwrap();
    let input = PayloadInput {
        from: &from,
        to: &to,
        cc: &[],
        subject: "rotation",
        message_id: &id,
        in_reply_to: None,
        references: &[],
        date: 1_793_000_000,
        text: &body,
        media_type: "text/plain",
        attachments: &[],
        autocrypt: &autocrypt,
    };
    let payload = build_payload(&input).unwrap();
    let now = unix_now();
    let targets = EncryptionTargets {
        visible: vec![usable_encryption_key(&bob_public, now).unwrap()],
        hidden: vec![],
    };
    let armored = sign_and_encrypt(&payload, &old, &targets).unwrap();
    smtp_raw(
        Who::Alice,
        Who::Bob,
        &build_outer(&input, &armored).unwrap(),
    )
    .await;

    let got = bob.wait_for(&body).await;
    assert_eq!(got.summary.encryption, Encryption::Openpgp);
    let held = bob.provider.peer(&from).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, new.fingerprint(), "rotated");
    assert_eq!(held.source, KeySource::AutocryptRotated);
}

/// The reviewer's attack, on the real server: a part Alice really signed, in a `multipart/signed` whose
/// root carries the attacker's key. The signature does not cover that root.
#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn the_root_of_a_multipart_signed_cannot_rotate_a_key() {
    let _one = serial().await;
    let b = tempfile::tempdir().unwrap();
    let bob = Person::open(Who::Bob, b.path(), Fetch::Imap, Submit::Smtp).await;
    let (alice, alice_rpgp) = crate::forge::key(&Who::Alice.address());
    let from = Who::Alice.address();
    bob.provider
        .import_peer_key(&from, &alice.public())
        .await
        .unwrap();
    let evil = generate_key(&from).unwrap();
    let t = token();
    let content = format!(
        "From: {from}\r\nSubject: Genuine{t}\r\nContent-Type: text/plain\r\n\r\nSigned-{t}\r\n"
    );
    let signature = crate::forge::detached(&alice_rpgp, &content);
    let head = format!(
        "From: {from}\r\nSubject: Evil{t}\r\nAutocrypt: {}\r\n",
        format_header(&from, &evil.public()).unwrap()
    );
    let payload = crate::forge::multipart_signed(&head, &content, &signature);
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let outer = format!(
        "From: {from}\r\nTo: {}\r\nSubject: [...]\r\nMessage-ID: <{t}@msgtest.test>\r\n",
        Who::Bob.address()
    );
    let raw = crate::forge::encrypted(&outer, payload.as_bytes(), &evil, &reader);
    smtp_raw(Who::Bob, Who::Bob, &raw).await;

    let got = bob.wait_for(&format!("Signed-{t}")).await;
    assert_eq!(got.summary.encryption, Encryption::Openpgp);
    let peer = bob.provider.peer(&from).await.unwrap().unwrap();
    assert_eq!(
        peer.fingerprint,
        alice.fingerprint(),
        "the key must not rotate"
    );
    assert_eq!(
        got.summary.envelope.name,
        format!("Genuine{t}"),
        "not the root's subject"
    );
}
