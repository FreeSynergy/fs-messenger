//! Keys that rotate, and keys that do not: who may replace a held key. A rotation signed by the v4
//! key beside a v6 key is a conflict, never a rotation, and a peer first seen with a v4 key gets its
//! v6 key only by that key's proof. Real keys and signatures through the real provider; only the
//! wire is replaced.
//!
//! Pattern: none (tests).

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/older.rs"]
mod older;

use common::{Offline, deliver, person};
use fs_messenger_mail::autocrypt::{format_header, format_header_with};
use fs_messenger_mail::openpgp::{Method, PublicCert, generate_key, generate_legacy_key};
use older::signed_mail;

const US: &str = "us@example.org";
const A: &str = "a@example.org";

async fn know(me: &Offline, address: &str, cert: &PublicCert) {
    me.provider.import_peer_key(address, cert).await.unwrap();
}

/// A mail from `from` to us: `outer` as its Autocrypt header, nothing signed.
fn plain_with(from: &str, outer: &str) -> Vec<u8> {
    format!("From: {from}\r\nTo: {US}\r\nSubject: hi\r\nMessage-ID: <p{}@e>\r\nAutocrypt: {outer}\r\n\r\nplain\r\n", outer.len()).into_bytes()
}

/// The v4 key beside a held v6 key is a way to reach the person, not a proof: it signs a payload that
/// offers a new key, and that is a conflict (the new key is a candidate), never a rotation.
#[tokio::test]
async fn a_rotation_signed_by_the_v4_key_beside_a_v6_key_is_a_conflict() {
    let me = person(US).await;
    let (a6, a4) = (forge::key(A), generate_legacy_key(A).unwrap());
    know(&me, A, &a6.0.public()).await;
    let ours = me.provider.ensure_own_key().await.unwrap().public();
    // The v4 key becomes known beside the v6 key, by the v6 key's own proof.
    let inner = format_header_with(A, &a4.public(), Some(&a6.0.public())).unwrap();
    deliver(&me, &signed_mail(&a6.0, &ours, &inner, "", Method::Modern)).await;
    assert_eq!(
        me.provider
            .peer(A)
            .await
            .unwrap()
            .unwrap()
            .legacy
            .unwrap()
            .fingerprint,
        a4.fingerprint()
    );
    // Now the v4 key signs an offer of a successor.
    let next = generate_key(A).unwrap();
    let offer = format_header(A, &next.public()).unwrap();
    deliver(&me, &signed_mail(&a4, &ours, &offer, "", Method::Older)).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(
        held.fingerprint,
        a6.0.fingerprint(),
        "the v6 key stays in use"
    );
    assert_eq!(
        held.candidate.map(|c| c.fingerprint),
        Some(next.fingerprint()),
        "the offer is visible, not used"
    );
}

/// A peer first seen through an older program (a v4 key only) is held as v4; its v6 key comes in
/// by the proof of that v4 key (a rotation), and without the proof it stays a candidate.
#[tokio::test]
async fn a_peer_first_seen_with_a_v4_key_gets_its_v6_key_only_by_proof() {
    let me = person(US).await;
    let (a4, a6) = (generate_legacy_key(A).unwrap(), generate_key(A).unwrap());
    deliver(
        &me,
        &plain_with(A, &format_header(A, &a4.public()).unwrap()),
    )
    .await;
    let ours = me.provider.ensure_own_key().await.unwrap().public();
    // Without proof (outer header only): a candidate.
    deliver(
        &me,
        &plain_with(A, &format_header(A, &a6.public()).unwrap()),
    )
    .await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, a4.fingerprint());
    assert_eq!(
        held.candidate.map(|c| c.fingerprint),
        Some(a6.fingerprint())
    );
    // Signed by a stranger's key: not a proof, the candidate stays a candidate.
    let stranger = generate_legacy_key(A).unwrap();
    let offer = format_header(A, &a6.public()).unwrap();
    deliver(
        &me,
        &signed_mail(&stranger, &ours, &offer, "", Method::Older),
    )
    .await;
    assert_eq!(
        me.provider.peer(A).await.unwrap().unwrap().fingerprint,
        a4.fingerprint(),
        "a valid signature of another key proves nothing"
    );
    // With the proof of the held v4 key: the v6 key is in use.
    let offer = format_header(A, &a6.public()).unwrap();
    deliver(&me, &signed_mail(&a4, &ours, &offer, "", Method::Older)).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, a6.fingerprint());
    assert_eq!(
        me.provider.encryption_method(&[A.into()]).await.unwrap(),
        Some(Method::Modern)
    );
}
