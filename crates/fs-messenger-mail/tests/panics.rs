//! A panic in a parser or in rPGP costs one step, not the mail, not the batch, not who sent it.

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/forge_subkeys.rs"]
mod forge_subkeys;

use common::{deliver, person, person_with_panicking_peers};
use forge::encrypted;
use forge_subkeys::unknown_aead;
use fs_messenger_core::Encryption;
use fs_messenger_mail::autocrypt::format_header;
use fs_messenger_mail::openpgp::generate_key;

const ALICE: &str = "a@example.org";
const BOB: &str = "b@example.org";

fn outer(id: &str) -> String {
    format!("From: {ALICE}\r\nTo: {BOB}\r\nSubject: [...]\r\nMessage-ID: <{id}@e>\r\n")
}

/// N6: an unknown AEAD byte opens the session key, and then rPGP panics. Who sent it stays known.
#[tokio::test]
async fn an_unknown_aead_byte_costs_the_content_and_not_who_sent_it() {
    let bob = person(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let alice = generate_key(ALICE).unwrap();
    let raw = unknown_aead(
        &outer("aead"),
        b"Content-Type: text/plain\r\n\r\nx\r\n",
        &alice,
        &reader,
    );
    let got = deliver(&bob, &raw).await;
    assert!(got.undecrypted);
    assert_eq!(got.encryption, Encryption::None);
    assert_eq!(got.sender, ALICE);
    assert_eq!(got.subject.as_deref(), Some("[...]"));
    assert_eq!(got.original.unwrap().bytes, raw);
}

/// The panic arm itself, with a panic we control.
#[tokio::test]
async fn a_panic_while_opening_keeps_the_outer_headers() {
    let bob = person_with_panicking_peers(BOB).await;
    let reader = bob.provider.ensure_own_key().await.unwrap().public();
    let alice = generate_key(ALICE).unwrap();
    let text = format!("From: {ALICE}\r\nContent-Type: text/plain\r\n\r\nx\r\n");
    let got = deliver(
        &bob,
        &encrypted(&outer("pan"), text.as_bytes(), &alice, &reader),
    )
    .await;
    assert!(got.undecrypted);
    assert_eq!(got.sender, ALICE);
    assert_eq!(got.links[0].id, "pan@e");
}

/// N7: learning from an offer runs in the contained path as well.
#[tokio::test]
async fn a_panic_while_learning_costs_the_offer_and_not_the_mail() {
    let bob = person_with_panicking_peers(BOB).await;
    let alice = generate_key(ALICE).unwrap().public();
    let header = format_header(ALICE, &alice).unwrap();
    let raw = format!("From: {ALICE}\r\nAutocrypt: {header}\r\nMessage-ID: <l@e>\r\n\r\nplain\r\n");
    let got = deliver(&bob, raw.as_bytes()).await;
    assert_eq!(got.body_text.trim_end(), "plain");
}
