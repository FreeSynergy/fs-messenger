//! Helpers for the tests of the older method (OpenPGP v4 keys, `SEIPDv1`): sending through the core,
//! reading the packets of what left, and a v4 key by rPGP in the legacy forms.

#![allow(dead_code)] // each test file uses a different subset

use fs_messenger_core::{Error, Messenger as _};
use fs_messenger_mail::openpgp::{Method, SecretCert, Shape};
use pgp::composed::SubkeyParamsBuilder;
use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SignedSecretKey};
use pgp::crypto::ecc_curve::ECCCurve;
use pgp::ser::Serialize as _;
use pgp::types::KeyVersion;

use crate::common::{Offline, core_with};

/// The armored message inside the `multipart/encrypted` of `raw`.
pub fn armor_of(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let start = text.find("-----BEGIN PGP MESSAGE-----").expect("armor");
    let end = text.find("-----END PGP MESSAGE-----").expect("armor end");
    text[start..end + 25].as_bytes().to_vec()
}

/// The packets of what `raw` carries.
pub fn shape_of(raw: &[u8]) -> Shape {
    Shape::of(&armor_of(raw))
}

/// Which method the packets make, asserting that they make a legal one.
pub fn method_of(raw: &[u8]) -> Method {
    let shape = shape_of(raw);
    shape
        .method()
        .unwrap_or_else(|| panic!("an illegal mix: {shape:?}"))
}

/// The real way out: `Draft.send` through the core. What the strategy got, or the core's refusal.
pub async fn send(me: &Offline, to: &[&str], bcc: &[&str]) -> Result<Vec<u8>, Error> {
    let (repo, account) = core_with(me).await;
    let owned = |l: &[&str]| l.iter().map(ToString::to_string).collect::<Vec<_>>();
    let mut d = repo.compose(None).await?;
    d.set_account(account).await?;
    d.set_recipients(owned(to), vec![], owned(bcc)).await?;
    d.set_subject("a quiet subject".into()).await?;
    d.set_text("the secret text".into(), "text/plain".into())
        .await?;
    d.send(None).await?;
    let mut sent = me.wire.sent.lock().unwrap();
    Ok(sent.pop().expect("something was submitted").1)
}

/// A v4 key in the legacy forms (EdDSA, ECDH Curve25519) built by hand, with the Features given.
pub fn legacy(address: &str, seipd_v1: bool, seipd_v2: bool) -> (SecretCert, SignedSecretKey) {
    let mut sub = SubkeyParamsBuilder::default();
    sub.version(KeyVersion::V4)
        .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
        .can_encrypt(EncryptionCaps::All);
    let mut key = SecretKeyParamsBuilder::default();
    key.version(KeyVersion::V4).key_type(KeyType::Ed25519Legacy);
    key.can_certify(true).can_sign(true);
    key.feature_seipd_v1(seipd_v1).feature_seipd_v2(seipd_v2);
    key.primary_user_id(format!("<{address}>"));
    key.subkey(sub.build().unwrap());
    let key = key.build().unwrap().generate(rand::thread_rng()).unwrap();
    (
        SecretCert::from_binary(&key.to_bytes().unwrap()).unwrap(),
        key,
    )
}
