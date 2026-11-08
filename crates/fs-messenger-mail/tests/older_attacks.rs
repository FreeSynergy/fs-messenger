//! The older method under attack, offline: a forged v4 offer cannot downgrade a peer who holds a v6 key,
//! first sight and rotation hold for v4 keys, `SEIPDv1` is read with its MDC, and when the older way
//! fails nothing leaves. Real keys and signatures through the real provider; only the wire is replaced.

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/older.rs"]
mod older;

use common::{Offline, deliver, person};
use fs_messenger_core::{Encryption, Error, SignatureCheck};
use fs_messenger_mail::MailError;
use fs_messenger_mail::autocrypt::{format_header, format_header_with};
use fs_messenger_mail::mime::{PayloadInput, build_outer, build_payload};
use fs_messenger_mail::openpgp::{
    EncryptionTargets, Method, PublicCert, SecretCert, decrypt, generate_key, generate_legacy_key,
    sign_and_encrypt, unix_now, usable_encryption_key,
};
use older::{legacy, method_of, send};

const US: &str = "us@example.org";
const A: &str = "a@example.org";
const B: &str = "b@example.org";

async fn know(me: &Offline, address: &str, cert: &PublicCert) {
    me.provider.import_peer_key(address, cert).await.unwrap();
}

async fn own_pair(p: &Offline) -> (SecretCert, SecretCert) {
    let modern = p.provider.ensure_own_key().await.unwrap();
    let older = p.keys.own_legacy_key(&p.address).await.unwrap().unwrap();
    (modern, older)
}

/// A mail from `from` to us: `outer` as its Autocrypt header, nothing signed.
fn plain_with(from: &str, outer: &str) -> Vec<u8> {
    format!("From: {from}\r\nTo: {US}\r\nSubject: hi\r\nMessage-ID: <p{}@e>\r\nAutocrypt: {outer}\r\n\r\nplain\r\n", outer.len()).into_bytes()
}

#[tokio::test]
async fn a_forged_v4_offer_never_downgrades_a_peer_who_holds_a_v6_key() {
    let me = person(US).await;
    let real = generate_key(A).unwrap();
    know(&me, A, &real.public()).await;
    let forger = generate_legacy_key(A).unwrap();
    // 1. v4 only, from outside.
    deliver(
        &me,
        &plain_with(A, &format_header(A, &forger.public()).unwrap()),
    )
    .await;
    // 2. the real v6 key with the forger's v4 key beside it, from outside.
    let both = format_header_with(A, &forger.public(), Some(&real.public())).unwrap();
    deliver(&me, &plain_with(A, &both)).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, real.fingerprint(), "the v6 key stays");
    assert!(held.legacy.is_none(), "no v4 key came in beside it");
    assert_eq!(
        held.candidate.unwrap().fingerprint,
        forger.fingerprint(),
        "visible, not used"
    );
    let raw = send(&me, &[A], &[]).await.unwrap();
    assert_eq!(method_of(&raw), Method::Modern, "still v2 to a v6 key");
    assert_eq!(
        me.provider.encryption_method(&[A.into()]).await.unwrap(),
        Some(Method::Modern)
    );
    assert!(
        decrypt(&older::armor_of(&raw), &[forger], &[], 1 << 20).is_err(),
        "he reads nothing"
    );
}

/// The positive side of the one above, so that it can fail: the v6 key vouches for a v4 key inside.
#[tokio::test]
async fn a_v4_key_beside_a_v6_one_comes_in_only_with_the_proof_of_the_v6_key() {
    let me = person(US).await;
    let (a6, a4) = (forge::key(A), generate_legacy_key(A).unwrap());
    know(&me, A, &a6.0.public()).await;
    let ours = me.provider.ensure_own_key().await.unwrap().public();
    let inner = format_header_with(A, &a4.public(), Some(&a6.0.public())).unwrap();
    // The v6 key signs, the header is inside, covered, header-protected: a proof.
    let mail = signed_mail(&a6.0, &ours, &inner, "", Method::Modern);
    deliver(&me, &mail).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.legacy.unwrap().fingerprint, a4.fingerprint());
    // A stranger's key signing the same does not do it, for a peer who has none yet.
    let me = person(US).await;
    know(&me, A, &a6.0.public()).await;
    let evil = generate_key(A).unwrap();
    let ours = me.provider.ensure_own_key().await.unwrap().public();
    deliver(&me, &signed_mail(&evil, &ours, &inner, "", Method::Modern)).await;
    assert!(me.provider.peer(A).await.unwrap().unwrap().legacy.is_none());
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

/// A mail from A to us signed by `signer`, with `inner` in the payload and `outer` outside.
fn signed_mail(
    signer: &SecretCert,
    reader: &PublicCert,
    inner: &str,
    outer: &str,
    method: Method,
) -> Vec<u8> {
    let to = vec![US.to_string()];
    let input = |autocrypt| PayloadInput {
        from: A,
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
    let key = usable_encryption_key(reader, unix_now()).unwrap();
    let targets = EncryptionTargets {
        visible: vec![key],
        hidden: vec![],
    };
    let armored = sign_and_encrypt(&payload, signer, &targets, method).unwrap();
    build_outer(&input(outer), &armored).unwrap()
}

#[tokio::test]
async fn first_seen_and_rotation_hold_for_v4_keys() {
    let me = person(US).await;
    let (good, evil, next) = (
        generate_legacy_key(A).unwrap(),
        generate_legacy_key(A).unwrap(),
        generate_legacy_key(A).unwrap(),
    );
    deliver(
        &me,
        &plain_with(A, &format_header(A, &good.public()).unwrap()),
    )
    .await;
    deliver(
        &me,
        &plain_with(A, &format_header(A, &evil.public()).unwrap()),
    )
    .await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(
        (
            held.fingerprint.as_str(),
            held.candidate.unwrap().fingerprint
        ),
        (good.fingerprint().as_str(), evil.fingerprint())
    );
    assert_eq!(
        me.provider.encryption_method(&[A.into()]).await.unwrap(),
        Some(Method::Older)
    );
    // An older client's key vouching for its successor inside an encrypted, signed mail: a rotation.
    let ours = me.provider.ensure_own_key().await.unwrap().public();
    let offer = format_header(A, &next.public()).unwrap();
    deliver(&me, &signed_mail(&good, &ours, &offer, "", Method::Older)).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, next.fingerprint());
}

#[tokio::test]
async fn incoming_seipd_v1_with_mdc_opens_with_our_v4_key_and_a_tampered_one_does_not() {
    let me = person(US).await;
    let (peer, _) = (generate_legacy_key(A).unwrap(), ());
    know(&me, A, &peer.public()).await;
    let (_, v4) = own_pair(&me).await;
    let named = signed_mail(&peer, &v4.public(), "", "", Method::Older);
    assert_eq!(method_of(&named), Method::Older);
    let got = deliver(&me, &named).await;
    assert_eq!(
        (got.encryption, got.signature),
        (Encryption::Openpgp, SignatureCheck::Valid)
    );
    assert_eq!(got.body_text.trim(), "text");
    // Flip one bit of the ciphertext, deep inside: the MDC (or the cipher) must refuse it.
    let text = String::from_utf8(named).unwrap();
    let armored = older::armor_of(text.as_bytes());
    let tampered = tamper(&armored);
    let wrapped = forge::wrapper(
        &format!("From: {A}\r\nTo: {US}\r\nMessage-ID: <t@e>\r\n"),
        &tampered,
    );
    let got = deliver(&me, &wrapped).await;
    assert_eq!(
        (got.encryption, got.undecrypted),
        (Encryption::None, true),
        "nothing of it is shown"
    );
    assert!(got.body_text.is_empty());
}

/// One bit flipped in the middle of the binary message, armored again.
fn tamper(armored: &[u8]) -> String {
    use base64::Engine as _;
    use std::io::Read as _;
    let mut binary = Vec::new();
    let reader = std::io::BufReader::new(armored);
    pgp::armor::Dearmor::new(reader)
        .read_to_end(&mut binary)
        .unwrap();
    let at = binary.len() * 3 / 4;
    binary[at] ^= 0x01;
    let text = base64::engine::general_purpose::STANDARD.encode(&binary);
    let lines: Vec<&str> = text
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN PGP MESSAGE-----\n\n{}\n-----END PGP MESSAGE-----\n",
        lines.join("\n")
    )
}

/// A store that cannot hand over our v4 key, as a failing disk would.
struct NoLegacy(
    std::sync::Arc<dyn fs_messenger_mail::keys::KeyStore>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
);

impl NoLegacy {
    fn failing(&self) -> Result<(), MailError> {
        let on = self.1.load(std::sync::atomic::Ordering::SeqCst);
        if on {
            Err(MailError::Keys("disk".into()))
        } else {
            Ok(())
        }
    }
}

#[async_trait::async_trait]
impl fs_messenger_mail::keys::KeyStore for NoLegacy {
    async fn own_key(&self, a: &str) -> Result<Option<SecretCert>, MailError> {
        self.0.own_key(a).await
    }
    async fn put_own_key(&self, a: &str, k: &SecretCert) -> Result<(), MailError> {
        self.0.put_own_key(a, k).await
    }
    async fn own_legacy_key(&self, a: &str) -> Result<Option<SecretCert>, MailError> {
        self.failing()?;
        self.0.own_legacy_key(a).await
    }
    async fn put_own_legacy_key(&self, a: &str, k: &SecretCert) -> Result<(), MailError> {
        self.failing()?;
        self.0.put_own_legacy_key(a, k).await
    }
    async fn peer(&self, a: &str) -> Result<Option<fs_messenger_mail::keys::PeerKey>, MailError> {
        self.0.peer(a).await
    }
    async fn put_peer(&self, p: &fs_messenger_mail::keys::PeerKey) -> Result<(), MailError> {
        self.0.put_peer(p).await
    }
}

#[tokio::test]
async fn when_the_older_way_fails_nothing_leaves_at_all() {
    let me = person(US).await;
    let b = legacy(B, true, false).0;
    know(&me, B, &b.public()).await;
    // 1. The key of the recipient is revoked between the plan and the send: refused, nothing submitted.
    let (_, forged) = forge::key(B);
    let revoked = forge::revoked(&forged);
    know(&me, B, &revoked).await;
    let Err(Error::EncryptionRefused { encryption, .. }) = send_err(&me, &[B]).await else {
        panic!("refused")
    };
    assert_eq!(encryption, Encryption::None);
    assert!(me.wire.sent.lock().unwrap().is_empty());
    // 2. Our own v4 key cannot be read: the older message cannot be signed or read by us.
    let down = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = down.clone();
    let wrap = move |inner| std::sync::Arc::new(NoLegacy(inner, flag)) as _;
    let broken = common::person_with_keys(US, wrap).await;
    down.store(true, std::sync::atomic::Ordering::SeqCst);
    know(&broken, B, &b.public()).await;
    assert!(send_err(&broken, &[B]).await.is_err());
    assert!(
        broken.wire.sent.lock().unwrap().is_empty(),
        "no fallback to something readable"
    );
    // 3. Everyone needs a key, v4 or not.
    assert!(send_err(&me, &["nobody@example.org"]).await.is_err());
    assert!(me.wire.sent.lock().unwrap().is_empty());
}

async fn send_err(me: &Offline, to: &[&str]) -> Result<Vec<u8>, Error> {
    send(me, to, &[]).await
}
