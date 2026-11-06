//! What an attacker can do with subkeys and packets: keys `generate_key` never makes, signatures by
//! subkeys, an AEAD byte no algorithm has.

#![allow(dead_code)] // each test file uses a different subset

use fs_messenger_mail::openpgp::{
    EncryptionTargets, PublicCert, SecretCert, sign_and_encrypt, unix_now, usable_encryption_key,
};
use pgp::composed::{
    ArmorOptions, DetachedSignature, EncryptionCaps, KeyType, MessageBuilder,
    SecretKeyParamsBuilder, SignedSecretKey, SubkeyParamsBuilder,
};
use pgp::crypto::aead::{AeadAlgorithm, ChunkSize};
use pgp::crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails as _, KeyVersion, Password, Timestamp};

use super::forge::wrapper;

/// A key with what `generate_key` never makes: subkeys `[encrypt, encrypt, sign, authenticate]`, and a
/// primary key that may sign only if `primary_signs`.
pub fn key_with_subkeys(address: &str, primary_signs: bool) -> (SecretCert, SignedSecretKey) {
    let sub = |key_type, encrypts: EncryptionCaps, signs, auth| {
        SubkeyParamsBuilder::default()
            .version(KeyVersion::V6)
            .key_type(key_type)
            .can_encrypt(encrypts)
            .can_sign(signs)
            .can_authenticate(auth)
            .build()
            .unwrap()
    };
    let key = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V6)
        .key_type(KeyType::Ed25519)
        .can_certify(true)
        .can_sign(primary_signs)
        .feature_seipd_v1(false)
        .feature_seipd_v2(true)
        .primary_user_id(format!("<{address}>"))
        .subkey(sub(KeyType::X25519, EncryptionCaps::All, false, false))
        .subkey(sub(KeyType::X25519, EncryptionCaps::All, false, false))
        .subkey(sub(KeyType::Ed25519, EncryptionCaps::None, true, false))
        .subkey(sub(KeyType::Ed25519, EncryptionCaps::None, false, true))
        .build()
        .unwrap()
        .generate(rand::thread_rng())
        .unwrap();
    (
        SecretCert::from_binary(&key.to_bytes().unwrap()).unwrap(),
        key,
    )
}

/// The certificate of `key`, with subkey `index` revoked by its owner, and the subkeys in `strip` gone.
pub fn subkey_revoked(key: &SignedSecretKey, index: usize, strip: &[usize]) -> PublicCert {
    let primary = &key.primary_key;
    let mut public = key.to_public_key();
    let mut config =
        SignatureConfig::from_key(rand::thread_rng(), primary, SignatureType::SubkeyRevocation)
            .unwrap();
    config.hashed_subpackets = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::now())).unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    let revocation = config
        .sign_subkey_binding(
            primary,
            primary.public_key(),
            &Password::empty(),
            &public.public_subkeys[index].key,
        )
        .unwrap();
    public.public_subkeys[index].signatures.push(revocation);
    let mut at = 0;
    public.public_subkeys.retain(|_| {
        at += 1;
        !strip.contains(&(at - 1))
    });
    PublicCert::from_binary(&public.to_bytes().unwrap()).unwrap()
}

/// `payload` signed with subkey `index` of `key`, encrypted to `reader`, wrapped under `head`.
pub fn encrypted_by_subkey(
    head: &str,
    payload: &[u8],
    key: &SignedSecretKey,
    index: usize,
    reader: &PublicCert,
) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut builder = MessageBuilder::from_bytes("", payload.to_vec()).seipd_v2(
        &mut rng,
        SymmetricKeyAlgorithm::AES256,
        AeadAlgorithm::Ocb,
        ChunkSize::default(),
    );
    let target = usable_encryption_key(reader, unix_now()).unwrap();
    builder.encrypt_to_key(&mut rng, target).unwrap();
    builder.sign(
        &key.secret_subkeys[index].key,
        Password::empty(),
        HashAlgorithm::Sha512,
    );
    let armored = builder
        .to_armored_string(&mut rng, ArmorOptions::default())
        .unwrap();
    wrapper(head, &armored)
}

/// A detached signature by subkey `index` of `key`.
pub fn detached_by_subkey(key: &SignedSecretKey, index: usize, content: &str) -> String {
    DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &key.secret_subkeys[index].key,
        &Password::empty(),
        HashAlgorithm::Sha512,
        content.as_bytes(),
    )
    .unwrap()
    .to_armored_string(ArmorOptions::default())
    .unwrap()
}

/// A real encrypted mail whose AEAD byte says something no algorithm is, as an attacker can write. The
/// session key still opens, so this reaches what comes after it. rPGP 0.21 panics there.
pub fn unknown_aead(
    head: &str,
    payload: &[u8],
    signer: &SecretCert,
    reader: &PublicCert,
) -> Vec<u8> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    let targets = EncryptionTargets {
        visible: vec![usable_encryption_key(reader, unix_now()).unwrap()],
        hidden: vec![],
    };
    let armored = sign_and_encrypt(payload, signer, &targets).unwrap();
    let body: String = armored
        .lines()
        .filter(|l| {
            !l.is_empty() && !l.starts_with("-----") && !l.starts_with('=') && !l.contains(": ")
        })
        .collect();
    let mut bytes = STANDARD.decode(body).unwrap();
    let at = seipd_body(&bytes);
    assert_eq!(bytes[at], 2, "SEIPDv2");
    bytes[at + 2] = 0x7f;
    let text = format!(
        "-----BEGIN PGP MESSAGE-----\n\n{}\n-----END PGP MESSAGE-----\n",
        STANDARD.encode(bytes)
    );
    wrapper(head, &text)
}

/// Where the body of the SEIPD packet (tag 18) starts, walking the new-format packets before it.
fn seipd_body(bytes: &[u8]) -> usize {
    let mut at = 0;
    loop {
        let tag = bytes[at] & 0x3f;
        let first = bytes[at + 1] as usize;
        let (header, length) = match first {
            0..=191 => (2, first),
            192..=223 => (3, ((first - 192) << 8) + bytes[at + 2] as usize + 192),
            255 => {
                let b = &bytes[at + 2..at + 6];
                (6, u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            }
            _ => (2, 0), // a partial length: only the packet we are looking for has one
        };
        if tag == 18 {
            return at + header;
        }
        at += header + length;
    }
}
