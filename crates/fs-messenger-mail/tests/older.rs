//! The older method, offline: an own v4 key beside the v6 one, `SEIPDv1` with `PKESKv3` to recipients
//! that cannot read v6, never mixed with the modern pair, and what an attacker cannot do to it. Real
//! keys and signatures, the real provider through the core's `Draft.send`; only the wire is replaced.

mod common;
#[path = "common/forge.rs"]
mod forge;
#[path = "common/older.rs"]
mod older;

use common::{Offline, deliver, person};
use fs_messenger_core::{Encryption, SignatureCheck};
use fs_messenger_mail::MailProvider;
use fs_messenger_mail::autocrypt::{AutocryptHeader, format_header_with};
use fs_messenger_mail::mime::MailFields;
use fs_messenger_mail::openpgp::{Method, PublicCert, SecretCert, decrypt, generate_key};
use older::{legacy, method_of, send, shape_of};

const US: &str = "us@example.org";
const A: &str = "a@example.org";
const B: &str = "b@example.org";

async fn know(me: &Offline, address: &str, cert: &PublicCert) {
    me.provider.import_peer_key(address, cert).await.unwrap();
}

async fn methods(
    me: &Offline,
    to: &[&str],
    bcc: &[&str],
) -> (Method, fs_messenger_mail::openpgp::Shape, Vec<u8>) {
    let raw = send(me, to, bcc).await.unwrap();
    (method_of(&raw), shape_of(&raw), raw)
}

async fn own_pair(p: &Offline) -> (SecretCert, SecretCert) {
    let modern = p.provider.ensure_own_key().await.unwrap();
    (
        modern,
        p.keys.own_legacy_key(&p.address).await.unwrap().unwrap(),
    )
}

/// Autocrypt 1.1 as a reader that knows nothing newer applies it: the attributes it knows, anything
/// starting with `_` ignored, anything else voiding the header. Returns the `keydata`.
fn strict_autocrypt_1_1(value: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let text = std::str::from_utf8(value).ok()?;
    let mut keydata = None;
    for attribute in text.split(';') {
        let (name, value) = attribute.split_once('=')?;
        match name.trim() {
            "addr" | "prefer-encrypt" => {}
            "keydata" => {
                let compact: String = value.split_whitespace().collect();
                keydata = base64::engine::general_purpose::STANDARD
                    .decode(compact)
                    .ok();
            }
            other if other.starts_with('_') => {}
            _ => return None,
        }
    }
    keydata
}

#[tokio::test]
async fn an_account_holds_two_keys_and_offers_the_v4_one_in_the_form_autocrypt_asks_for() {
    let (me, bob) = (person(US).await, person(A).await);
    let (v6, v4) = own_pair(&me).await;
    assert!(v6.public().is_v6() && v4.public().is_v4());
    let packets =
        pgp::packet::PacketParser::new(std::io::Cursor::new(v4.public().to_binary().unwrap()));
    assert_eq!(
        packets.count(),
        5,
        "key, user id, its signature, subkey, its binding"
    );
    know(
        &me,
        A,
        &bob.provider.ensure_own_key().await.unwrap().public(),
    )
    .await;
    let raw = send(&me, &[A], &[]).await.unwrap();
    let value = MailFields::read(&raw).unwrap().autocrypt.remove(0);
    let keydata = strict_autocrypt_1_1(&value).expect("an old reader accepts the header");
    assert_eq!(
        keydata,
        v4.public().to_binary().unwrap(),
        "keydata is the v4 key"
    );
    let parsed = AutocryptHeader::parse(&value).unwrap();
    assert_eq!(parsed.v6keydata.unwrap(), v6.public().to_binary().unwrap());
    // A reader of ours that sees only `keydata` holds the v4 key; one that sees `_fs-v6` the v6 key.
    assert!(String::from_utf8_lossy(&value).contains("_fs-v6="));
}

#[tokio::test]
async fn the_method_follows_the_weakest_reader_and_never_mixes() {
    let me = person(US).await;
    let (v6, v4) = (generate_key(A).unwrap(), legacy(B, true, false).0);
    let (also_v4, _) = legacy("c@example.org", true, false);
    let v4_modern = legacy("d@example.org", true, true).0;
    for (who, key) in [
        (A, &v6),
        (B, &v4),
        ("c@example.org", &also_v4),
        ("d@example.org", &v4_modern),
    ] {
        know(&me, who, &key.public()).await;
    }
    let (m, shape, _) = methods(&me, &[A], &[]).await;
    assert_eq!(
        (m, shape.pkesk.len()),
        (Method::Modern, 2),
        "v6 to v6, us included"
    );
    assert!(shape.pkesk.iter().all(|v| *v == 6) && shape.data == Some((18, 2)));
    let (m, shape, _) = methods(&me, &[B], &[]).await;
    assert_eq!(m, Method::Older, "a v4 key without the v2 flag");
    assert!(shape.pkesk.iter().all(|v| *v == 3) && shape.data == Some((18, 1)));
    let (m, _, _) = methods(&me, &[A, B], &[]).await;
    assert_eq!(m, Method::Older, "one old reader turns it older for all");
    let (m, shape, _) = methods(&me, &[A], &[B]).await;
    assert_eq!(
        (m, shape.pkesk.len()),
        (Method::Older, 3),
        "a hidden reader counts as well"
    );
    let (m, _, _) = methods(&me, &["d@example.org", A], &[]).await;
    assert_eq!(
        m,
        Method::Modern,
        "a v4 key that says SEIPDv2 may get it (RFC 9580 13.7)"
    );
    assert_eq!(
        me.provider.encryption_method(&[A.into()]).await.unwrap(),
        Some(Method::Modern)
    );
    assert_eq!(
        me.provider
            .encryption_method(&[A.into(), B.into()])
            .await
            .unwrap(),
        Some(Method::Older)
    );
    assert_eq!(
        me.provider
            .encryption_method(&["nobody@example.org".into()])
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn an_older_message_reads_for_each_of_its_readers_and_the_sent_copy_stays_readable() {
    let me = person(US).await;
    let (a, b) = (generate_key(A).unwrap(), legacy(B, true, false).0);
    know(&me, A, &a.public()).await;
    know(&me, B, &b.public()).await;
    let raw = send(&me, &[A, B], &[]).await.unwrap();
    assert_eq!(method_of(&raw), Method::Older);
    // The v6 reader gets a v3 PKESK to her v6 key, and reads it (RFC 9580 5.1.1 does not forbid it).
    for key in [&a, &b] {
        let (plain, state) = decrypt(
            &older::armor_of(&raw),
            std::slice::from_ref(key),
            &[],
            1 << 20,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&plain).contains("the secret text"));
        let _ = state;
    }
    // The sent copy: ours, through the real reading flow, with the v4 key we signed with.
    let copy = deliver(&me, &raw).await;
    assert_eq!(copy.encryption, Encryption::Openpgp);
    assert_eq!(copy.body_text.trim(), "the secret text");
    assert_eq!(
        copy.signature,
        SignatureCheck::Valid,
        "signed by our v4 key, known to us"
    );
    let (_, v4) = own_pair(&me).await;
    assert!(decrypt(&older::armor_of(&raw), &[v4], &[], 1 << 20).is_ok());
    // The frontend's bridge: the original says it is the older method, and the plan says so ahead.
    assert_eq!(MailProvider::method_of_original(&raw), Some(Method::Older));
}

#[tokio::test]
async fn a_peer_with_both_keys_gets_the_v4_one_in_an_older_message() {
    let me = person(US).await;
    let (a6, a4) = (generate_key(A).unwrap(), legacy(A, true, false).0);
    let b = legacy(B, true, false).0;
    // First sight of the pair, in one header.
    let header = format_header_with(A, &a4.public(), Some(&a6.public())).unwrap();
    let wire =
        format!("From: {A}\r\nTo: {US}\r\nSubject: x\r\nAutocrypt: {header}\r\n\r\nplain\r\n");
    deliver(&me, wire.as_bytes()).await;
    let held = me.provider.peer(A).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, a6.fingerprint());
    assert_eq!(held.legacy.as_ref().unwrap().fingerprint, a4.fingerprint());
    know(&me, B, &b.public()).await;
    let raw = send(&me, &[A, B], &[]).await.unwrap();
    assert_eq!(method_of(&raw), Method::Older);
    let shape_ids = decrypt(
        &older::armor_of(&raw),
        std::slice::from_ref(&a4),
        &[],
        1 << 20,
    );
    assert!(shape_ids.is_ok(), "her v4 key opens it");
    assert!(matches!(
        send(&me, &[A], &[]).await.map(|r| method_of(&r)),
        Ok(Method::Modern)
    ));
}
