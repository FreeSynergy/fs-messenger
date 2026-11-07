//! What an attacker can write: mails by hand, with real keys and real signatures.

#![allow(dead_code)] // each test file uses a different subset

use fs_messenger_mail::openpgp::{
    EncryptionTargets, Method, PublicCert, SecretCert, sign_and_encrypt, unix_now,
    usable_encryption_key,
};
use pgp::composed::{
    ArmorOptions, DetachedSignature, EncryptionCaps, KeyType, MessageBuilder,
    SecretKeyParamsBuilder, SignedSecretKey, SubkeyParamsBuilder,
};
use pgp::crypto::aead::{AeadAlgorithm, ChunkSize};
use pgp::crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::ser::Serialize as _;
use pgp::types::{CompressionAlgorithm, KeyDetails as _, KeyVersion, Password, Timestamp};

/// A key, in the form this crate takes and in the form rPGP signs with.
pub fn key(address: &str) -> (SecretCert, SignedSecretKey) {
    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::X25519)
        .can_encrypt(EncryptionCaps::All)
        .build()
        .unwrap();
    let key = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_certify(true)
        .can_sign(true)
        .feature_seipd_v1(false)
        .feature_seipd_v2(true)
        .primary_user_id(format!("<{address}>"))
        .subkey(subkey)
        .build()
        .unwrap()
        .generate(rand::thread_rng())
        .unwrap();
    (
        SecretCert::from_binary(&key.to_bytes().unwrap()).unwrap(),
        key,
    )
}

/// The certificate of `key`, revoked by its owner.
pub fn revoked(key: &SignedSecretKey) -> PublicCert {
    let primary = &key.primary_key;
    let mut config =
        SignatureConfig::from_key(rand::thread_rng(), primary, SignatureType::KeyRevocation)
            .unwrap();
    config.hashed_subpackets = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::now())).unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    let signature = config
        .sign_key(primary, &Password::empty(), primary.public_key())
        .unwrap();
    let mut public = key.to_public_key();
    public.details.revocation_signatures.push(signature);
    PublicCert::from_binary(&public.to_bytes().unwrap()).unwrap()
}

/// A detached signature over `content`, as a real mail program makes it.
pub fn detached(key: &SignedSecretKey, content: &str) -> String {
    DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &key.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha512,
        content.as_bytes(),
    )
    .unwrap()
    .to_armored_string(ArmorOptions::default())
    .unwrap()
}

/// RFC 3156 section 5, with `head` above it, which the signature does not cover.
pub fn multipart_signed(head: &str, content: &str, signature: &str) -> String {
    let signature = signature.replace("\r\n", "\n").replace('\n', "\r\n");
    format!(
        "{head}Content-Type: multipart/signed; micalg=pgp-sha512; \
protocol=\"application/pgp-signature\"; boundary=\"s\"\r\n\r\n--s\r\n{content}\r\n--s\r\n\
Content-Type: application/pgp-signature\r\n\r\n{signature}\r\n--s--\r\n"
    )
}

/// RFC 3156 section 4 around an armored message, with `head` above it.
pub fn wrapper(head: &str, armored: &str) -> Vec<u8> {
    let armored = armored.replace("\r\n", "\n").replace('\n', "\r\n");
    format!(
        "{head}MIME-Version: 1.0\r\nContent-Type: multipart/encrypted; \
protocol=\"application/pgp-encrypted\"; boundary=\"b\"\r\n\r\n--b\r\n\
Content-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\r\n--b\r\n\
Content-Type: application/octet-stream\r\n\r\n{armored}\r\n--b--\r\n"
    )
    .into_bytes()
}

/// `payload` signed by `signer`, encrypted to `reader`, wrapped under `head`.
pub fn encrypted(head: &str, payload: &[u8], signer: &SecretCert, reader: &PublicCert) -> Vec<u8> {
    let targets = EncryptionTargets {
        visible: vec![usable_encryption_key(reader, unix_now()).unwrap()],
        hidden: vec![],
    };
    wrapper(
        head,
        &sign_and_encrypt(payload, signer, &targets, Method::Modern).unwrap(),
    )
}

/// `size` zero bytes, compressed and encrypted to `reader`: a small mail that expands.
pub fn bomb(head: &str, size: usize, reader: &PublicCert) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut builder = MessageBuilder::from_bytes("", vec![0u8; size]).seipd_v2(
        &mut rng,
        SymmetricKeyAlgorithm::AES256,
        AeadAlgorithm::Ocb,
        ChunkSize::default(),
    );
    builder.compression(CompressionAlgorithm::ZIP);
    let key = usable_encryption_key(reader, unix_now()).unwrap();
    builder.encrypt_to_key(&mut rng, key).unwrap();
    let armored = builder
        .to_armored_string(&mut rng, ArmorOptions::default())
        .unwrap();
    wrapper(head, &armored)
}

/// Every form a secret key could be found in, for needle checks: the bare scalar, as bytes and hex, in
/// base64 at all three alignments, and the whole secret key in base64 as it is stored.
pub fn secret_needles(key: &SignedSecretKey) -> Vec<Vec<u8>> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    let whole = key.to_bytes().unwrap();
    let public = key.primary_key.public_params().to_bytes().unwrap();
    let at = whole
        .windows(public.len())
        .position(|w| w == public)
        .expect("the public part");
    // A v6 Ed25519 secret key packet: the public part, one byte for "no S2K", the 32 bytes of the scalar.
    let scalar = &whole[at + public.len() + 1..at + public.len() + 33];
    let hex = scalar.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    });
    let mut needles = vec![
        scalar.to_vec(),
        hex.into_bytes(),
        STANDARD.encode(&whole).into_bytes(),
    ];
    for skip in 0..3 {
        let text = STANDARD.encode(&scalar[skip..]);
        needles.push(text.as_bytes()[..text.len() - 4].to_vec());
    }
    needles
}

/// Whether any needle is in `haystack`.
pub fn holds_any(haystack: &[u8], needles: &[Vec<u8>]) -> bool {
    needles
        .iter()
        .any(|n| haystack.windows(n.len()).any(|w| w == n.as_slice()))
}

/// All bytes of all files in `dir`: the database, and its log files if there are any.
pub fn files_of(dir: &std::path::Path) -> Vec<u8> {
    let mut all = vec![];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            all.extend(std::fs::read(path).unwrap());
        }
    }
    all
}
