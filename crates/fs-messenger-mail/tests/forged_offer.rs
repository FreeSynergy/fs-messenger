//! The hard cases of the Autocrypt rule, through the real reading flow: a real signature does not vouch
//! for a header it does not cover, and a new key cannot vouch for itself.

mod common;
#[path = "common/older.rs"]
mod older;

use common::{Offline, person};
use fs_messenger_core::{Encryption, Provider};
use fs_messenger_mail::autocrypt::format_header;
use fs_messenger_mail::mime::{PayloadInput, build_outer, build_payload};
use fs_messenger_mail::openpgp::{
    EncryptionTargets, Method, PublicCert, SecretCert, generate_key, sign_and_encrypt, unix_now,
    usable_encryption_key,
};

const ALICE: &str = "a@example.org";
const BOB: &str = "b@example.org";

/// A mail signed by `signer`, with `inner` in the payload and `outer` on the outside.
fn mail(signer: &SecretCert, reader: &PublicCert, inner: &str, outer: &str) -> Vec<u8> {
    let to = vec![BOB.to_string()];
    let input = |autocrypt| PayloadInput {
        from: ALICE,
        to: &to,
        cc: &[],
        subject: "s",
        message_id: "m@example.org",
        in_reply_to: None,
        references: &[],
        date: 1_793_000_000,
        text: "text",
        media_type: "text/plain",
        attachments: &[],
        autocrypt,
    };
    let payload = build_payload(&input(inner)).unwrap();
    let targets = EncryptionTargets {
        visible: vec![usable_encryption_key(reader, unix_now()).unwrap()],
        hidden: vec![],
    };
    let armored = sign_and_encrypt(&payload, signer, &targets, Method::Modern).unwrap();
    build_outer(&input(outer), &armored).unwrap()
}

async fn deliver(bob: &Offline, raw: Vec<u8>) {
    bob.wire.inbox.lock().unwrap().push(raw);
    let got = bob
        .provider
        .fetch(None)
        .await
        .unwrap()
        .messages
        .pop()
        .unwrap();
    assert_eq!(
        got.encryption,
        Encryption::Openpgp,
        "the case needs a mail that really opened"
    );
}

async fn setup() -> (Offline, SecretCert, PublicCert, SecretCert) {
    let bob = person(BOB).await;
    let alice = generate_key(ALICE).unwrap();
    bob.provider
        .import_peer_key(ALICE, &alice.public())
        .await
        .unwrap();
    let own = bob.provider.ensure_own_key().await.unwrap().public();
    let evil = generate_key(ALICE).unwrap();
    (bob, alice, own, evil)
}

/// Alice really signed the mail and wrote no inner header, as other mail programs do. An attacker adds
/// an outer one. The signature verifies, and says nothing about it.
#[tokio::test]
async fn a_good_signature_does_not_cover_an_outer_header() {
    let (bob, alice, own, evil) = setup().await;
    let forged = format_header(ALICE, &evil.public()).unwrap();
    deliver(&bob, mail(&alice, &own, "", &forged)).await;
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, alice.fingerprint(), "the known key stays");
    assert_eq!(held.candidate.unwrap().fingerprint, evil.fingerprint());
}

/// The attacker has bob's public key, so he can write an encrypted mail. Its inner header offers his
/// key and his key signs it: that opens, and verifies with his key only.
#[tokio::test]
async fn a_new_key_cannot_vouch_for_itself() {
    let (bob, alice, own, evil) = setup().await;
    let header = format_header(ALICE, &evil.public()).unwrap();
    deliver(&bob, mail(&evil, &own, &header, &header)).await;
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, alice.fingerprint(), "the known key stays");
    assert_eq!(held.candidate.unwrap().fingerprint, evil.fingerprint());
}

/// The positive side, so that the two above can fail: the known key vouches for the new one inside.
#[tokio::test]
async fn the_known_key_vouching_inside_rotates() {
    let (bob, alice, own, next) = setup().await;
    let header = format_header(ALICE, &next.public()).unwrap();
    deliver(&bob, mail(&alice, &own, &header, "")).await;
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, next.fingerprint());
    assert!(held.candidate.is_none());
}

/// The key beside the key in use (v4) is held and may sign, but it cannot vouch for a new key: a weaker
/// key must not rotate the stronger one. Twin of the test above, with the legacy key as the signer.
#[tokio::test]
async fn the_legacy_key_cannot_rotate_the_key_in_use() {
    let (bob, alice, own, next) = setup().await;
    let (legacy, _) = older::legacy(ALICE, true, false);
    let mut peer = bob.keys.peer(ALICE).await.unwrap().unwrap();
    peer.legacy = Some(fs_messenger_mail::keys::LegacyKey::new(&legacy.public()).unwrap());
    bob.keys.put_peer(&peer).await.unwrap();
    let header = format_header(ALICE, &next.public()).unwrap();
    deliver(&bob, mail(&legacy, &own, &header, "")).await;
    let held = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(
        held.fingerprint,
        alice.fingerprint(),
        "the key in use stays"
    );
    assert_eq!(held.candidate.unwrap().fingerprint, next.fingerprint());
}
