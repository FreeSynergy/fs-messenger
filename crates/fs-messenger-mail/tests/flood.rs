//! Third review: what anyone can write without a key, in the unhashed area of a copied signature.

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/forge_subkeys.rs"]
mod forge_subkeys;

use common::{Offline, deliver, person};
use forge::key;
use forge_subkeys::{key_with_subkeys, padded, padded_subkey_signature, subkey_revoked};
use fs_messenger_mail::autocrypt::{Decision, KeyLearner, Offer, format_header};
use fs_messenger_mail::openpgp::{PublicCert, unix_now, usable_encryption_key};

const ALICE: &str = "a@example.org";
const BOB: &str = "b@example.org";

fn forged(n: u32, form: &PublicCert) -> Vec<u8> {
    format!(
        "From: {ALICE}\r\nMessage-ID: <{n}@e>\r\nAutocrypt: {}\r\n\r\nforged\r\n",
        format_header(ALICE, form).unwrap()
    )
    .into_bytes()
}

async fn held_bytes(bob: &Offline) -> usize {
    let peer = bob.provider.peer(ALICE).await.unwrap().unwrap();
    peer.certificate().unwrap().to_binary().unwrap().len()
}

/// What the reviewer ran: 40 mails, each with new padding, grew a certificate from 720 bytes to 2271 KiB.
#[tokio::test]
async fn forty_forged_mails_do_not_grow_a_known_key() {
    let bob = person(BOB).await;
    let (alice, _) = key(ALICE);
    bob.provider
        .import_peer_key(ALICE, &alice.public())
        .await
        .unwrap();
    let before = bob.provider.peer(ALICE).await.unwrap().unwrap();
    for n in 0..40 {
        let form = padded(&alice.public(), &format!("n{n}"), 40_000);
        deliver(&bob, &forged(n, &form)).await;
    }
    let after = bob.provider.peer(ALICE).await.unwrap().unwrap();
    assert_eq!(after.cert, before.cert, "nothing new was learned");
    assert_eq!(after.candidate, None);
}

/// Padded on first contact: what is kept is what the signatures cover.
#[tokio::test]
async fn padding_on_first_contact_is_not_kept() {
    let bob = person(BOB).await;
    let (alice, _) = key(ALICE);
    deliver(&bob, &forged(1, &padded(&alice.public(), "first", 40_000))).await;
    assert!(
        held_bytes(&bob).await < 4000,
        "{} bytes",
        held_bytes(&bob).await
    );
}

/// A real change still comes through, also inside a padded form, and without the padding.
#[tokio::test]
async fn a_real_revocation_in_a_padded_form_is_taken_without_the_padding() {
    let bob = person(BOB).await;
    let (_, rpgp) = key_with_subkeys(ALICE, true);
    let whole =
        PublicCert::from_binary(&pgp::ser::Serialize::to_bytes(&rpgp.to_public_key()).unwrap())
            .unwrap();
    bob.provider.import_peer_key(ALICE, &whole).await.unwrap();
    let before = held_bytes(&bob).await;
    let learner = KeyLearner::new(bob.keys.clone());
    let revoked = subkey_revoked(&rpgp, 0, &[]);
    // The padding is on the very signature that is new, and on another one.
    let offer = Offer {
        cert: padded_subkey_signature(&padded(&revoked, "real", 40_000), 0, 40_000),
        proven_by_known_key: false,
    };
    let decision = learner.learn(ALICE, &offer).await.unwrap();
    assert_eq!(decision, Decision::SameKey { take_offered: true });
    assert!(
        held_bytes(&bob).await < before + 500,
        "{} bytes",
        held_bytes(&bob).await
    );
    let held = bob
        .provider
        .peer(ALICE)
        .await
        .unwrap()
        .unwrap()
        .certificate()
        .unwrap();
    assert!(usable_encryption_key(&held, unix_now()).is_ok());
}
