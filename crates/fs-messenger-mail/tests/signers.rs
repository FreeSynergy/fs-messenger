//! Second review: who may sign, and what counts as having signed a header. Real keys with subkeys
//! `generate_key` never makes, real signatures, the real reading flow; only the wire is replaced.

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/forge_subkeys.rs"]
mod forge_subkeys;

use common::{Offline, deliver, person};
use forge::{encrypted, multipart_signed};
use forge_subkeys::{detached_by_subkey, encrypted_by_subkey, key_with_subkeys, subkey_revoked};
use fs_messenger_core::{Encryption, SignatureCheck};
use fs_messenger_mail::autocrypt::{Decision, KeyLearner, Offer, format_header};
use fs_messenger_mail::keys::{KeySource, PeerKey};
use fs_messenger_mail::openpgp::{PublicCert, generate_key, unix_now, usable_encryption_key};

const ALICE: &str = "a@example.org";
const BOB: &str = "b@example.org";
const SIGNING: usize = 2;
const AUTH: usize = 3;

fn outer(id: &str) -> String {
    format!("From: {ALICE}\r\nTo: {BOB}\r\nSubject: [...]\r\nMessage-ID: <{id}@e>\r\n")
}

/// A protected payload offering `header`.
fn protected(header: &str) -> String {
    format!(
        "From: {ALICE}\r\nAutocrypt: {header}\r\nContent-Type: text/plain; hp=\"cipher\"\r\n\r\nnew key\r\n"
    )
}

async fn bob_holding(cert: &PublicCert) -> (Offline, PublicCert) {
    let bob = person(BOB).await;
    bob.provider.import_peer_key(ALICE, cert).await.unwrap();
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    (bob, reader)
}

async fn held(bob: &Offline) -> String {
    bob.provider.peer(ALICE).await.unwrap().unwrap().fingerprint
}

fn next_key() -> (String, String) {
    let next = generate_key(ALICE).unwrap().public();
    (next.fingerprint(), format_header(ALICE, &next).unwrap())
}

/// The positive side, so that the refusals below can fail: a live signing subkey does prove a rotation.
#[tokio::test]
async fn a_live_signing_subkey_proves_a_rotation() {
    let (alice, rpgp) = key_with_subkeys(ALICE, true);
    let (bob, reader) = bob_holding(&alice.public()).await;
    let (next, header) = next_key();
    let raw = encrypted_by_subkey(
        &outer("live"),
        protected(&header).as_bytes(),
        &rpgp,
        SIGNING,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::Valid);
    assert_eq!(held(&bob).await, next);
}

/// N1, what the reviewer ran: the owner revoked the signing subkey, somebody holds it and signs a rotation.
#[tokio::test]
async fn a_revoked_signing_subkey_proves_nothing() {
    let (alice, rpgp) = key_with_subkeys(ALICE, true);
    let (bob, reader) = bob_holding(&subkey_revoked(&rpgp, SIGNING, &[])).await;
    let (next, header) = next_key();
    let raw = encrypted_by_subkey(
        &outer("rev"),
        protected(&header).as_bytes(),
        &rpgp,
        SIGNING,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.encryption, Encryption::Openpgp);
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(
        held(&bob).await,
        alice.fingerprint(),
        "the key must not rotate"
    );
    let peer = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(peer.candidate.unwrap().fingerprint, next);
}

/// The same with the detached path.
#[tokio::test]
async fn a_revoked_signing_subkey_proves_nothing_when_detached() {
    let (alice, rpgp) = key_with_subkeys(ALICE, true);
    let (bob, reader) = bob_holding(&subkey_revoked(&rpgp, SIGNING, &[])).await;
    let (_, header) = next_key();
    let content = protected(&header);
    let signature = detached_by_subkey(&rpgp, SIGNING, &content);
    let payload = multipart_signed("", &content, &signature);
    let stranger = generate_key("s@example.org").unwrap();
    let raw = encrypted(&outer("revd"), payload.as_bytes(), &stranger, &reader);
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(held(&bob).await, alice.fingerprint());
}

/// An authentication subkey has no business signing mail, even though it is the right algorithm.
#[tokio::test]
async fn a_subkey_without_the_sign_flag_signs_nothing() {
    let (alice, rpgp) = key_with_subkeys(ALICE, true);
    let (bob, reader) = bob_holding(&alice.public()).await;
    let (_, header) = next_key();
    let raw = encrypted_by_subkey(
        &outer("auth"),
        protected(&header).as_bytes(),
        &rpgp,
        AUTH,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(held(&bob).await, alice.fingerprint());
}

/// A primary key that only certifies signs no mail either.
#[tokio::test]
async fn a_primary_key_without_the_sign_flag_signs_nothing() {
    let (alice, _) = key_with_subkeys(ALICE, false);
    let (bob, reader) = bob_holding(&alice.public()).await;
    let (_, header) = next_key();
    let raw = encrypted(
        &outer("prim"),
        protected(&header).as_bytes(),
        &alice,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert_eq!(got.signature, SignatureCheck::NotVerified);
    assert_eq!(held(&bob).await, alice.fingerprint());
}

/// N2, what the reviewer ran: Alice signs prose. `mail-parser` skips the first line and reads the rest as
/// headers, so her text offered a key.
#[tokio::test]
async fn prose_with_header_lines_is_prose() {
    let alice = generate_key(ALICE).unwrap();
    let (bob, reader) = bob_holding(&alice.public()).await;
    let (_, header) = next_key();
    let prose =
        format!("Dear all\nFrom: {ALICE}\nSubject: quoted subject\nAutocrypt: {header}\n\nbye\n");
    let raw = encrypted(&outer("prose"), prose.as_bytes(), &alice, &reader);
    let got = deliver(&bob, &raw).await;
    assert_eq!(
        held(&bob).await,
        alice.fingerprint(),
        "the key must not rotate"
    );
    assert_eq!(
        got.signature,
        SignatureCheck::NotVerified,
        "it names no sender"
    );
    assert_eq!(got.subject.as_deref(), Some("[...]"), "not the quoted one");
    assert!(
        got.body_text.contains("Dear all"),
        "and all of it is the text"
    );
    assert!(got.body_text.contains("quoted subject"));
}

/// Prose that happens to be a clean block still declares nothing: a proof needs `hp` on that very part.
#[tokio::test]
async fn a_clean_block_without_hp_proves_no_rotation() {
    let alice = generate_key(ALICE).unwrap();
    let (bob, reader) = bob_holding(&alice.public()).await;
    let (next, header) = next_key();
    let text = format!("From: {ALICE}\r\nAutocrypt: {header}\r\n\r\nbye\r\n");
    let raw = encrypted(&outer("nohp"), text.as_bytes(), &alice, &reader);
    let got = deliver(&bob, &raw).await;
    assert_eq!(
        got.signature,
        SignatureCheck::Valid,
        "she did sign it, and it names her"
    );
    assert_eq!(held(&bob).await, alice.fingerprint());
    let peer = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(
        peer.candidate.unwrap().fingerprint,
        next,
        "it stays an offer"
    );
}

/// N5: the real revocation of one subkey, with the other one stripped, must not leave the peer unusable.
#[tokio::test]
async fn a_form_with_a_subkey_stripped_takes_nothing_away() {
    let bob = person(BOB).await;
    let (_, rpgp) = key_with_subkeys(ALICE, true);
    let whole = subkey_revoked(&rpgp, 3, &[]);
    let learner = KeyLearner::new(bob.keys.clone());
    let peer = PeerKey::new(ALICE, &whole, KeySource::Person, unix_now()).unwrap();
    bob.keys.put_peer(&peer).await.unwrap();

    let offer = Offer {
        cert: subkey_revoked(&rpgp, 1, &[0]),
        companion: None,
        proven_by_known_key: false,
    };
    let decision = learner.learn(ALICE, &offer).await.unwrap();
    assert_eq!(
        decision,
        Decision::SameKey { take_offered: true },
        "the revocation is new"
    );
    let merged = bob
        .keys
        .peer(ALICE)
        .await
        .unwrap()
        .unwrap()
        .certificate()
        .unwrap();
    assert!(
        usable_encryption_key(&merged, unix_now()).is_ok(),
        "subkey 0 is still there"
    );

    // And that revocation holds: only subkey 0 is left to encrypt to.
    let again = learner
        .learn(
            ALICE,
            &Offer {
                cert: subkey_revoked(&rpgp, 0, &[1]),
                companion: None,
                proven_by_known_key: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(again, Decision::SameKey { take_offered: true });
    let merged = bob
        .keys
        .peer(ALICE)
        .await
        .unwrap()
        .unwrap()
        .certificate()
        .unwrap();
    assert!(
        usable_encryption_key(&merged, unix_now()).is_err(),
        "both revoked now, for real"
    );
}
