//! What arrives: real crypto through the real reading flow; only the wire is replaced.

mod common;

use common::{Offline, deliver, person};
use fs_messenger_core::{Encryption, SignatureCheck};
use fs_messenger_mail::autocrypt::format_header;
use fs_messenger_mail::mime::{OUTER_SUBJECT, PayloadInput, build_outer, build_payload};
use fs_messenger_mail::openpgp::{
    EncryptionTargets, Method, SecretCert, sign_and_encrypt, unix_now, usable_encryption_key,
};

async fn own(p: &Offline) -> SecretCert {
    p.provider.ensure_own_key().await.unwrap()
}

/// A finished mail from `from` to `to`, signed by `signer`, with `header` as its key offer.
fn mail(from: &str, to: &str, signer: &SecretCert, reader: &SecretCert, header: &str) -> Vec<u8> {
    let list = vec![to.to_string()];
    let input = PayloadInput {
        from,
        to: &list,
        cc: &[],
        subject: "real subject",
        message_id: "m1@example.org",
        in_reply_to: None,
        references: &[],
        date: 1_793_000_000,
        text: "the secret text\n",
        media_type: "text/plain",
        attachments: &[],
        autocrypt: header,
    };
    let payload = build_payload(&input).unwrap();
    let public = reader.public();
    let targets = EncryptionTargets {
        visible: vec![usable_encryption_key(&public, unix_now()).unwrap()],
        hidden: vec![],
    };
    build_outer(
        &input,
        &sign_and_encrypt(&payload, signer, &targets, Method::Modern).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn decrypted_means_openpgp_with_protected_headers_and_the_original_untouched() {
    let (alice, bob) = (person("a@example.org").await, person("b@example.org").await);
    let (ka, kb) = (own(&alice).await, own(&bob).await);
    let header = format_header(&alice.address, &ka.public()).unwrap();
    let raw = mail(&alice.address, &bob.address, &ka, &kb, &header);
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.encryption, Encryption::Openpgp);
    assert_eq!(got.body_text, "the secret text\n");
    assert_eq!(
        got.subject.as_deref(),
        Some("real subject"),
        "from inside, not {OUTER_SUBJECT}"
    );
    assert_eq!(
        got.aliases[0].scheme, "mail-local",
        "only what the provider issued dedupes"
    );
    assert_eq!(
        got.links[0].id, "m1@example.org",
        "the sender's id only links"
    );
    assert_eq!(got.original.unwrap().bytes, raw, "byte for byte (ADR 0030)");
    // The key came with this mail, so it proves nothing yet. The next one is signed by a known key.
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(deliver(&bob, &raw).await.signature, SignatureCheck::Valid);
}

#[tokio::test]
async fn a_mail_for_someone_else_stays_undecrypted_with_its_original() {
    let (alice, bob, eve) = (
        person("a@example.org").await,
        person("b@example.org").await,
        person("e@example.org").await,
    );
    let (ka, kb) = (own(&alice).await, own(&bob).await);
    let header = format_header(&alice.address, &ka.public()).unwrap();
    let raw = mail(&alice.address, &eve.address, &ka, &kb, &header);
    let got = deliver(&eve, &raw).await;
    assert_eq!(got.encryption, Encryption::None);
    assert!(got.undecrypted);
    assert!(
        got.body_text.is_empty(),
        "no text pretends to be the message"
    );
    assert_eq!(got.subject.as_deref(), Some(OUTER_SUBJECT));
    assert!(got.attachments.is_empty());
    assert_eq!(got.original.unwrap().bytes, raw);
}

#[tokio::test]
async fn damaged_armor_is_kept_and_marked_and_no_error_loses_it() {
    let bob = person("b@example.org").await;
    let raw = b"From: a@example.org\r\nMessage-ID: <x@e>\r\nContent-Type: multipart/encrypted; \
protocol=\"application/pgp-encrypted\"; boundary=y\r\n\r\n--y\r\nContent-Type: application/pgp-encrypted\r\n\r\n\
Version: 1\r\n--y\r\nContent-Type: application/octet-stream\r\n\r\nnot pgp at all\r\n--y--\r\n";
    let got = deliver(&bob, raw).await;
    assert_eq!(got.encryption, Encryption::None);
    assert!(got.undecrypted);
    assert_eq!(got.original.unwrap().bytes, raw);
}

#[tokio::test]
async fn garbage_is_stored_too_under_an_alias_of_its_own() {
    let bob = person("b@example.org").await;
    let got = deliver(&bob, b"\x00\x01 no header at all").await;
    assert_eq!(got.encryption, Encryption::None);
    assert!(got.aliases[0].id.starts_with("b@example.org/box/1/"));
    assert!(got.original.is_some());
}

#[tokio::test]
async fn a_mail_without_message_id_has_a_stable_alias_from_account_mailbox_and_uid() {
    let bob = person("b@example.org").await;
    let got = deliver(&bob, b"From: a@example.org\r\n\r\nhello\r\n").await;
    assert_eq!(got.aliases.len(), 1);
    assert_eq!(got.aliases[0].id, "b@example.org/box/1/1");
}

#[tokio::test]
async fn plain_text_around_a_ciphertext_is_plain() {
    let (alice, bob) = (person("a@example.org").await, person("b@example.org").await);
    let (ka, kb) = (own(&alice).await, own(&bob).await);
    let header = format_header(&alice.address, &ka.public()).unwrap();
    let inner = mail(&alice.address, &bob.address, &ka, &kb, &header);
    let at = inner.windows(10).position(|w| w == b"-----BEGIN").unwrap();
    let armor = String::from_utf8_lossy(&inner[at..]).into_owned();
    let end = armor.find("-----END PGP MESSAGE-----").unwrap() + 25;
    let raw = format!(
        "From: a@example.org\r\nMessage-ID: <p@e>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nReadable and added by anybody\r\n\
--x\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=y\r\n\r\n\
--y\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\
--y\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--y--\r\n--x--\r\n",
        &armor[..end]
    );
    let got = deliver(&bob, raw.as_bytes()).await;
    assert_eq!(
        got.encryption,
        Encryption::None,
        "only the root can make a mail encrypted"
    );
}

#[tokio::test]
async fn a_literal_packet_in_an_encrypted_envelope_is_not_openpgp() {
    use pgp::composed::{ArmorOptions, MessageBuilder};
    let bob = person("b@example.org").await;
    let literal = MessageBuilder::from_bytes("", b"plain inside".to_vec())
        .to_armored_string(rand::thread_rng(), ArmorOptions::default())
        .unwrap();
    let raw = format!(
        "From: a@example.org\r\nMessage-ID: <l@e>\r\nContent-Type: multipart/encrypted; \
protocol=\"application/pgp-encrypted\"; boundary=y\r\n\r\n--y\r\nContent-Type: application/pgp-encrypted\r\n\r\n\
Version: 1\r\n--y\r\nContent-Type: application/octet-stream\r\n\r\n{literal}\r\n--y--\r\n"
    );
    let got = deliver(&bob, raw.as_bytes()).await;
    assert_eq!(got.encryption, Encryption::None);
    assert!(got.undecrypted);
}

#[tokio::test]
async fn a_signature_by_a_stranger_does_not_read_as_valid() {
    let (alice, bob, eve) = (
        person("a@example.org").await,
        person("b@example.org").await,
        person("e@example.org").await,
    );
    let (ka, kb, ke) = (own(&alice).await, own(&bob).await, own(&eve).await);
    bob.provider
        .import_peer_key(&alice.address, &ka.public())
        .await
        .unwrap();
    let header = format_header(&alice.address, &ka.public()).unwrap();
    let raw = mail(&alice.address, &bob.address, &ke, &kb, &header);
    let got = deliver(&bob, &raw).await;
    assert_eq!(
        got.encryption,
        Encryption::Openpgp,
        "it was decrypted; that is all the state says"
    );
    assert_eq!(got.signature, SignatureCheck::NotVerified);
}

#[tokio::test]
async fn a_mail_never_counts_as_mine_because_of_its_from() {
    let bob = person("b@example.org").await;
    let got = deliver(
        &bob,
        b"From: b@example.org\r\nMessage-ID: <f@e>\r\n\r\nspoof\r\n",
    )
    .await;
    assert!(!got.outgoing, "only the sent mailbox can say that");
    assert!(
        got.links.is_empty(),
        "and no link that could match a receipt of ours"
    );
}
