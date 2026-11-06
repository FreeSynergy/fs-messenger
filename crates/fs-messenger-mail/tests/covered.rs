//! What a signature covers decides what it may vouch for. Real keys, signatures and reading flow.

mod common;
#[path = "common/forge.rs"]
mod forge;

use common::{Offline, deliver, person};
use forge::{bomb, detached, encrypted, key, multipart_signed, revoked};
use fs_messenger_core::{Encryption, SignatureCheck};
use fs_messenger_mail::autocrypt::format_header;
use fs_messenger_mail::openpgp::generate_key;

const ALICE: &str = "a@example.org";
const BOB: &str = "b@example.org";
const EVIL_SUBJECT: &str = "Subject: pay this invoice\r\n";

fn outer(from: &str, id: &str) -> String {
    format!("From: {from}\r\nTo: {BOB}\r\nSubject: [...]\r\nMessage-ID: <{id}>\r\n")
}

fn genuine(from_header: bool) -> String {
    let from = if from_header {
        format!("From: {ALICE}\r\n")
    } else {
        String::new()
    };
    format!("{from}Subject: genuine\r\nContent-Type: text/plain\r\n\r\nAlice wrote this\r\n")
}

async fn bob_knowing_alice() -> (
    Offline,
    fs_messenger_mail::openpgp::SecretCert,
    pgp::composed::SignedSecretKey,
) {
    let bob = person(BOB).await;
    let (alice, alice_rpgp) = key(ALICE);
    bob.provider
        .import_peer_key(ALICE, &alice.public())
        .await
        .unwrap();
    (bob, alice, alice_rpgp)
}

/// The reviewer's attack: a real part signed by Alice, in a `multipart/signed` whose ROOT carries the
/// attacker's key. The signature verifies, and covers none of the root.
#[tokio::test]
async fn the_root_of_a_multipart_signed_proves_nothing() {
    let (bob, alice, alice_rpgp) = bob_knowing_alice().await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let evil = generate_key(ALICE).unwrap();
    let content = genuine(true);
    let around = format!(
        "From: {ALICE}\r\n{EVIL_SUBJECT}Autocrypt: {}\r\n",
        format_header(ALICE, &evil.public()).unwrap()
    );
    let payload = multipart_signed(&around, &content, &detached(&alice_rpgp, &content));
    let raw = encrypted(&outer(ALICE, "root@e"), payload.as_bytes(), &evil, &reader);

    let got = deliver(&bob, &raw).await;
    assert_eq!(got.encryption, Encryption::Openpgp);
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(
        held.fingerprint,
        alice.fingerprint(),
        "the key must not rotate"
    );
    assert_eq!(
        got.signature,
        SignatureCheck::Valid,
        "the signed part does verify"
    );
    assert_eq!(
        got.subject.as_deref(),
        Some("genuine"),
        "not the root's subject"
    );
    assert_eq!(got.body_text, "Alice wrote this\n");
}

/// A key offered inside the signed part IS covered: that is the rotation.
#[tokio::test]
async fn an_offer_inside_the_signed_part_rotates() {
    let (bob, _alice, alice_rpgp) = bob_knowing_alice().await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let next = generate_key(ALICE).unwrap();
    let header = format_header(ALICE, &next.public()).unwrap();
    let content = format!(
        "From: {ALICE}\r\nAutocrypt: {header}\r\nContent-Type: text/plain; hp=\"cipher\"\r\n\r\nnew key\r\n"
    );
    let payload = multipart_signed("", &content, &detached(&alice_rpgp, &content));
    let raw = encrypted(&outer(ALICE, "in@e"), payload.as_bytes(), &next, &reader);
    deliver(&bob, &raw).await;
    assert_eq!(
        bob.provider.peer(ALICE).await.unwrap().unwrap().fingerprint,
        next.fingerprint()
    );
}

/// A signature over a bare body says nothing about who sent it: anyone can put it under any `From`.
#[tokio::test]
async fn a_signed_part_without_a_from_is_not_valid_for_the_sender() {
    let (bob, _alice, alice_rpgp) = bob_knowing_alice().await;
    let content = genuine(false);
    let raw = multipart_signed(
        &outer(ALICE, "bare@e"),
        &content,
        &detached(&alice_rpgp, &content),
    );
    let got = deliver(&bob, raw.as_bytes()).await;
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(got.encryption, Encryption::None);
}

#[tokio::test]
async fn headers_shown_next_to_valid_are_the_signed_ones() {
    let (bob, _alice, alice_rpgp) = bob_knowing_alice().await;
    let content = genuine(true);
    let head = format!("From: {ALICE}\r\n{EVIL_SUBJECT}Message-ID: <plain@e>\r\n");
    let raw = multipart_signed(&head, &content, &detached(&alice_rpgp, &content));
    let got = deliver(&bob, raw.as_bytes()).await;
    assert_eq!(got.signature, SignatureCheck::Valid);
    assert_eq!(got.subject.as_deref(), Some("genuine"));
}

/// Without `hp`, a valid signature still covers the inner headers; the unsigned outer ones must lose.
#[tokio::test]
async fn signed_inner_headers_win_over_unsigned_outer_ones_without_hp() {
    let (bob, alice, _) = bob_knowing_alice().await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let head = format!("From: {ALICE}\r\n{EVIL_SUBJECT}Message-ID: <hp@e>\r\n");
    let raw = encrypted(&head, genuine(true).as_bytes(), &alice, &reader);
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::Valid);
    assert_eq!(got.subject.as_deref(), Some("genuine"));
}

/// Only `cipher` and `clear` declare anything; an unsigned inner header under another value is not
/// believed.
#[tokio::test]
async fn an_unknown_hp_value_declares_nothing() {
    let bob = person(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let stranger = generate_key("s@example.org").unwrap();
    let inner = format!(
        "From: {ALICE}\r\nSubject: inner\r\nContent-Type: multipart/mixed; hp=\"nonsense\"; boundary=\"x\"\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\ntext\r\n--x--\r\n"
    );
    let head = format!("From: {BOB}\r\nSubject: outer\r\nMessage-ID: <w@e>\r\n");
    let got = deliver(
        &bob,
        &encrypted(&head, inner.as_bytes(), &stranger, &reader),
    )
    .await;
    assert_eq!(got.subject.as_deref(), Some("outer"));
    let protected = inner.replace("nonsense", "cipher");
    let head = format!("From: {BOB}\r\nSubject: outer\r\nMessage-ID: <c@e>\r\n");
    let got = deliver(
        &bob,
        &encrypted(&head, protected.as_bytes(), &stranger, &reader),
    )
    .await;
    assert_eq!(got.subject.as_deref(), Some("inner"));
}

/// The outer header was checked against the outer `From` and stored for the inner one.
#[tokio::test]
async fn an_outer_offer_counts_only_for_the_final_sender() {
    let bob = person(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let (mallory, carol) = ("m@example.org", "c@example.org");
    let evil = generate_key(mallory).unwrap();
    let head = format!(
        "From: {mallory}\r\nMessage-ID: <o@e>\r\nAutocrypt: {}\r\n",
        format_header(mallory, &evil.public()).unwrap()
    );
    let inner = format!(
        "From: {carol}\r\nContent-Type: multipart/mixed; hp=\"cipher\"; boundary=\"x\"\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\ntext\r\n--x--\r\n"
    );
    deliver(&bob, &encrypted(&head, inner.as_bytes(), &evil, &reader)).await;
    assert!(
        bob.provider.peer(carol).await.unwrap().is_none(),
        "no key of mallory's under carol"
    );
}

/// A held key that was revoked proves nothing, and its signature does not read valid.
#[tokio::test]
async fn a_revoked_key_proves_no_rotation() {
    let bob = person(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let (alice, alice_rpgp) = key(ALICE);
    bob.provider
        .import_peer_key(ALICE, &revoked(&alice_rpgp))
        .await
        .unwrap();
    let next = generate_key(ALICE).unwrap();
    let header = format_header(ALICE, &next.public()).unwrap();
    let inner = format!(
        "From: {ALICE}\r\nAutocrypt: {header}\r\nContent-Type: multipart/mixed; hp=\"cipher\"; boundary=\"x\"\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nt\r\n--x--\r\n"
    );
    let raw = encrypted(&outer(ALICE, "rev@e"), inner.as_bytes(), &alice, &reader);
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, alice.fingerprint());
    assert_eq!(held.candidate.unwrap().fingerprint, next.fingerprint());
}

/// A replay of my own signed mail under a new outer `Message-ID` links by the id I signed.
#[tokio::test]
async fn a_replay_of_my_own_ciphertext_links_by_the_inner_id() {
    let me = person(ALICE).await;
    let own = me.provider.ensure_own_key().await.unwrap();
    let inner = format!(
        "From: {ALICE}\r\nMessage-ID: <real@e>\r\nContent-Type: multipart/mixed; hp=\"cipher\"; boundary=\"x\"\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nmine\r\n--x--\r\n"
    );
    let raw = encrypted(
        &outer(ALICE, "replayed@e"),
        inner.as_bytes(),
        &own,
        &own.public(),
    );
    let got = deliver(&me, &raw).await;
    assert_eq!(got.signature, SignatureCheck::Valid);
    assert_eq!(
        got.links[0].id, "real@e",
        "not the outer id an attacker chose"
    );
}

/// A small mail that expands past the limit stays undecrypted, keeps its original, and the next goes on.
#[tokio::test]
async fn a_compression_bomb_stays_undecrypted_and_the_next_mail_arrives() {
    let bob = common::person_limited(BOB, 1024 * 1024).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let raw = bomb(&outer(ALICE, "bomb@e"), 16 * 1024 * 1024, &reader);
    assert!(
        raw.len() < 1024 * 1024,
        "the mail itself is under the limit: {}",
        raw.len()
    );
    let got = deliver(&bob, &raw).await;
    assert!(got.undecrypted);
    assert_eq!(got.encryption, Encryption::None);
    assert_eq!(got.body_text, "");
    assert_eq!(got.original.unwrap().bytes, raw);
    let next = format!("{}\r\nnext\r\n", outer(ALICE, "next@e"));
    assert_eq!(
        deliver(&bob, next.as_bytes()).await.body_text.trim_end(),
        "next"
    );
}

/// A panic while reading costs that mail its content, not the mail and not the rest of the batch.
#[tokio::test]
async fn a_panic_while_reading_leaves_the_mail_stored_and_undecrypted() {
    let bob = common::person_with_panicking_peers(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let (alice, _) = key(ALICE);
    let raw = encrypted(
        &outer(ALICE, "panic@e"),
        genuine(true).as_bytes(),
        &alice,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert!(got.undecrypted);
    assert_eq!(got.encryption, Encryption::None);
    assert_eq!(got.original.unwrap().bytes, raw);
}

/// Only the start of a mail over the limit was read: it says who and what, and teaches no key.
#[tokio::test]
async fn a_cut_off_mail_offers_no_key_and_is_not_opened() {
    let bob = person(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let (alice, _) = key(ALICE);
    let head = format!(
        "{}Autocrypt: {}\r\n",
        outer(ALICE, "cut@e"),
        format_header(ALICE, &alice.public()).unwrap()
    );
    let raw = encrypted(&head, genuine(true).as_bytes(), &alice, &reader);
    bob.wire
        .cut_off
        .store(9_000_000, std::sync::atomic::Ordering::SeqCst);
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.encryption, Encryption::None, "nothing was decrypted");
    assert_eq!(got.too_large, Some(9_000_000), "it says what it holds");
    assert_eq!(got.original, None, "a start is no original");
    assert_eq!(got.subject.as_deref(), Some("[...]"));
    assert_eq!(got.body_text, "");
    assert!(
        bob.provider.peer(ALICE).await.unwrap().is_none(),
        "no key learned"
    );
}
