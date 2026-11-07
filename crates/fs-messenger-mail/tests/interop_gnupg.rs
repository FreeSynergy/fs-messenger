//! Interoperability with `GnuPG`: real round trips through the real provider; only the wire is replaced.
//! The results, and how RNP was measured the same way: `DESIGN.md`, "Interoperability".
//! Pattern: none (tests). Every gpg gets a new throwaway home (`--homedir`, `GNUPGHOME`), whose agent is
//! killed and whose directory is deleted on drop; the real `~/.gnupg` is never read.
//!
//! Run: `TMPDIR=<short dir> cargo test -p fs-messenger-mail --test interop_gnupg -- --ignored --nocapture
//! --test-threads=1`. Skips with a message if gpg is missing. The assertions pin `GnuPG` 2.4.9 of Fedora 45:
//! failing with another build means `DESIGN.md` is out of date, not that the code broke.

mod common;
#[path = "common/gpg.rs"]
mod gpg;

use std::io::Cursor;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::{Offline, person};
use fs_messenger_core::*;
use fs_messenger_mail::autocrypt::AutocryptHeader;
use fs_messenger_mail::mime::MailFields;
use fs_messenger_mail::openpgp::*;
use gpg::Gpg;
use pgp::composed::{Deserializable as _, EncryptionCaps, KeyType as K, SecretKeyParamsBuilder};
use pgp::composed::{SignedPublicKey, SubkeyParamsBuilder};
use pgp::{crypto::ecc_curve::ECCCurve, ser::Serialize as _, types::KeyVersion};

const US: &str = "us@example.org";
const PEER: &str = "peer@example.org";
const BODY: &str = "Content-Type: text/plain\r\n\r\nthe secret text\r\n";
/// As gpg users sign it when their program protects headers: the signature covers the sender.
const NAMED: &str = "From: peer@example.org\r\nContent-Type: text/plain\r\n\r\nthe secret text\r\n";

/// A v4 secret key by rPGP that advertises `SEIPDv2`: the one kind of v4 key the plan passes.
fn v4_with_seipd2(address: &str, modern: bool) -> Vec<u8> {
    let legacy = K::ECDH(ECCCurve::Curve25519Legacy);
    let sub = if modern { K::X25519 } else { legacy };
    let primary = if modern { K::Ed25519 } else { K::Ed25519Legacy };
    let mut s = SubkeyParamsBuilder::default();
    s.version(KeyVersion::V4).key_type(sub);
    s.can_encrypt(EncryptionCaps::All);
    let mut p = SecretKeyParamsBuilder::default();
    p.version(KeyVersion::V4).key_type(primary);
    p.can_certify(true).can_sign(true);
    p.feature_seipd_v1(true).feature_seipd_v2(true);
    p.primary_user_id(format!("<{address}>"));
    p.subkey(s.build().unwrap());
    let digests = [pgp::crypto::hash::HashAlgorithm::Sha512];
    p.preferred_hash_algorithms(digests.into_iter().collect());
    let key = p.build().unwrap().generate(rand::thread_rng()).unwrap();
    key.to_bytes().unwrap()
}

/// The real way out: `Draft.send` through the core; what the strategy got is the mail.
async fn send(me: &Offline, bcc: &[String]) -> Result<Vec<u8>> {
    let store: Arc<dyn fs_store::Store> = Arc::new(fs_store::TursoStore::open_in_memory().await?);
    let config = MessengerConfig::new("https://local.invalid/me");
    let repo = MessengerRepo::open(store, config).await?;
    let (address, kind) = (me.address.clone(), "mail".to_string());
    let secret = String::new();
    let setup = AccountSetup {
        address,
        kind,
        secret,
    };
    let account = repo.add_account(setup).await?;
    repo.register_provider(account.id, me.provider.clone());
    let mut d = repo.compose(None).await?;
    d.set_account(account.id).await?;
    let to = vec![PEER.to_string()];
    d.set_recipients(to, vec![], bcc.to_vec()).await?;
    d.set_text("the secret text".into(), "text/plain".into())
        .await?;
    d.send(None).await?;
    Ok(me.wire.sent.lock().unwrap().pop().unwrap().1)
}

fn armor_of(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let start = text.find("-----BEGIN").unwrap();
    let end = text.find("-----END PGP MESSAGE-----").unwrap();
    text[start..end + 25].as_bytes().to_vec()
}

/// RFC 3156 section 4 by hand, as another program writes it, not by our builder.
fn wrapped(armored: &[u8], head: &str) -> Vec<u8> {
    let armored = String::from_utf8_lossy(armored).replace('\n', "\r\n");
    format!(
        "From: {PEER}\r\nTo: {US}\r\nMessage-ID: <i@e>\r\n{head}MIME-Version: 1.0\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\n{armored}\r\n--b--\r\n"
    ).into_bytes()
}

/// RFC 3156 section 5 around `body`, with `head` above it.
fn signed(signature: &[u8], head: &str, body: &str) -> Vec<u8> {
    let signature = String::from_utf8_lossy(signature);
    format!(
        "{head}Content-Type: multipart/signed; micalg=pgp-sha512; protocol=\"application/pgp-signature\"; boundary=\"s\"\r\n\r\n--s\r\n{body}\r\n--s\r\nContent-Type: application/pgp-signature\r\n\r\n{signature}\r\n--s--\r\n"
    ).into_bytes()
}

/// Through the real reading flow: what the core gets, and rPGP's error.
async fn read(p: &Offline, label: &str, raw: &[u8]) -> (Encryption, SignatureCheck) {
    p.wire.inbox.lock().unwrap().push(raw.to_vec());
    let mut batch = p.provider.fetch(None).await.unwrap();
    let got = batch.messages.pop().unwrap();
    let (encryption, body, signature) = (got.encryption, got.body_text, got.signature);
    println!("{label}: {encryption:?} {signature:?} {body:?}");
    if got.undecrypted {
        let own = p.keys.own_key(US).await.unwrap().unwrap();
        let error = decrypt(&armor_of(raw), &[own], &[], 1 << 24).err().unwrap();
        println!("  decrypt: {error}");
    }
    (encryption, signature)
}

async fn own(p: &Offline) -> SecretCert {
    p.provider.ensure_own_key().await.unwrap()
}

async fn plan(me: &Offline) -> EncryptionPlan {
    let who = [PEER.to_string()];
    me.provider.encryption_plan(&who).await.unwrap()
}

#[tokio::test]
#[ignore = "needs gpg; see the module doc"]
async fn a_f_our_certificate_and_autocrypt_header() {
    let Some(g) = Gpg::new() else { return };
    g.run("--version", b"");
    let (me, bob) = (person(US).await, person(PEER).await);
    let bytes = own(&me).await.public().to_binary().unwrap();
    g.run("--list-packets", &bytes);
    assert!(!g.run("--import", &bytes).0);
    assert!(!g.run(&format!("--encrypt --recipient {US}"), b"x").0);
    let key = own(&bob).await.public();
    me.provider.import_peer_key(PEER, &key).await.unwrap();
    let raw = send(&me, &[]).await.unwrap();
    let header = MailFields::read(&raw).unwrap().autocrypt.remove(0);
    println!("F: value of {} octets", header.len());
    let keydata = AutocryptHeader::parse(&header).unwrap().keydata;
    assert_eq!(keydata, bytes);
    assert!(!g.run("--import", &keydata).0);
    assert!(!g.opens(&armor_of(&raw)), "C1: mail to a v6 key");
}

#[tokio::test]
#[ignore = "needs gpg; see the module doc"]
async fn b_c_a_gpg_key_and_our_mail() {
    let Some(g) = Gpg::new() else { return };
    let me = person(US).await;
    g.generate(PEER, "");
    let export = g.run(&format!("--export {PEER}"), b"").1;
    g.run("--list-packets", &export);
    let cert = PublicCert::from_binary(&export).unwrap();
    let refusal = usable_encryption_key(&cert, unix_now()).err().unwrap();
    assert_eq!(refusal, KeyRefusal::NoModernEncryption);
    me.provider.import_peer_key(PEER, &cert).await.unwrap();
    let answer = plan(&me).await;
    println!("B: {refusal:?}: {refusal}\nB: {answer:?}");
    println!("B: {:?}", send(&me, &[]).await.err());
    assert_eq!(answer.encryption, Encryption::None);
    assert_eq!(answer.missing_keys, [PEER]);
    // C4, not reachable in the product, the plan refuses this key: the real function, past the plan.
    let key = SignedPublicKey::from_bytes(Cursor::new(&export)).unwrap();
    let (visible, hidden) = (vec![&key.public_subkeys[0]], vec![]);
    let targets = EncryptionTargets { visible, hidden };
    let armored = sign_and_encrypt(BODY.as_bytes(), &own(&me).await, &targets);
    assert!(!g.opens(armored.unwrap().as_bytes()), "C4");
    for modern in [false, true] {
        let g = Gpg::new().unwrap();
        let secret = v4_with_seipd2(PEER, modern);
        assert_eq!(g.run("--import", &secret).0, !modern);
        let public = SecretCert::from_binary(&secret).unwrap().public();
        me.provider.import_peer_key(PEER, &public).await.unwrap();
        assert_eq!(plan(&me).await.encryption, Encryption::Openpgp);
        assert!(!g.opens(&armor_of(&send(&me, &[]).await.unwrap())));
        let hidden = [PEER.to_string()];
        assert!(!g.opens(&armor_of(&send(&me, &hidden).await.unwrap())));
    }
}

#[tokio::test]
#[ignore = "needs gpg; see the module doc"]
async fn d_e_gpg_mail_to_a_key_of_ours() {
    for made in [None, Some(v4_with_seipd2(US, false))] {
        let Some(g) = Gpg::new() else { return };
        g.generate(PEER, "");
        let secret = made.unwrap_or_else(|| {
            g.generate(US, "");
            g.run(&format!("--export-secret-keys {US}"), b"").1
        });
        g.run("--import", &secret);
        let export = g.run(&format!("--export {PEER}"), b"").1;
        let (known, unknown) = (person(US).await, person(US).await);
        let key = SecretCert::from_binary(&secret).unwrap();
        let signer = PublicCert::from_binary(&export).unwrap();
        known.keys.put_own_key(US, &key).await.unwrap();
        unknown.keys.put_own_key(US, &key).await.unwrap();
        known.provider.import_peer_key(PEER, &signer).await.unwrap();
        let data = STANDARD.encode(&export);
        let offer = format!("Autocrypt: addr={PEER}; keydata={data}\r\n");
        let base = format!("--armor --sign --encrypt --local-user {PEER} --recipient {US}");
        for extra in ["", " --rfc4880", " --force-ocb", " --digest-algo SHA1"] {
            let armored = g.run(&format!("{base}{extra}"), NAMED.as_bytes()).1;
            g.run("--list-packets", &armored);
            let raw = wrapped(&armored, "");
            let got = read(&known, "known", &raw).await;
            if extra == " --force-ocb" {
                assert_eq!(got, (Encryption::None, SignatureCheck::NotVerified));
                continue;
            }
            let sha1 = extra.contains("SHA1");
            assert_eq!(got.0, Encryption::Openpgp);
            assert_eq!(got.1 == SignatureCheck::Valid, !sha1);
            // A signature over a bare body does not vouch for the sender.
            let unnamed = g.run(&format!("{base}{extra}"), BODY.as_bytes()).1;
            let got = read(&known, "bare body", &wrapped(&unnamed, "")).await;
            assert_eq!(got, (Encryption::Openpgp, SignatureCheck::NotVerified));
            let got = read(&unknown, "unknown", &raw).await;
            assert_eq!(got.1, SignatureCheck::NotVerified);
            // A key offered in the same mail proves nothing about that mail.
            let got = read(&unknown, "offered", &wrapped(&armored, &offer)).await;
            assert_eq!(got.1, SignatureCheck::NotVerified);
            assert!(unknown.keys.peer(PEER).await.unwrap().is_none());
        }
        let detach = format!("--armor --detach-sign --local-user {PEER}");
        let signature = g.run(&detach, NAMED.as_bytes()).1;
        let encrypt = format!("--armor --encrypt --recipient {US}");
        let inner = g.run(&encrypt, &signed(&signature, "", NAMED)).1;
        let got = read(&known, "RFC 3156 6.1", &wrapped(&inner, "")).await;
        assert_eq!(got, (Encryption::Openpgp, SignatureCheck::Valid));
        let head = format!("From: {PEER}\r\nTo: {US}\r\nMessage-ID: <s@e>\r\n");
        let got = read(&known, "RFC 3156 5", &signed(&signature, &head, NAMED)).await;
        assert_eq!(got, (Encryption::None, SignatureCheck::Valid));
        let unnamed = g.run(&detach, BODY.as_bytes()).1;
        let got = read(&known, "RFC 3156 5, bare", &signed(&unnamed, &head, BODY)).await;
        assert_eq!(got, (Encryption::None, SignatureCheck::NotVerified));
    }
    let g = Gpg::new().unwrap();
    g.generate(US, "Key-Version: 5\nSubkey-Version: 5\n");
    let export = g.run(&format!("--export {US}"), b"").1;
    g.run("--list-packets", &export);
    println!("E: {}", PublicCert::from_binary(&export).err().unwrap());
    let secret = g.run(&format!("--export-secret-keys {US}"), b"").1;
    assert!(SecretCert::from_binary(&secret).is_err());
}
