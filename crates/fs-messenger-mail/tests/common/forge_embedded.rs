//! What anyone can write into an embedded signature, and a back-signature where other programs put it.

#![allow(dead_code)] // each test file uses a different subset

use fs_messenger_mail::openpgp::PublicCert;
use pgp::composed::SignedSecretKey;
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails as _, Password, Timestamp};

/// A signature nobody signed, with `size` bytes in its hashed area, which stripping an outer signature's
/// unhashed area leaves alone unless the embedded signature goes too.
fn junk_embedded(model: &pgp::packet::Signature, size: usize) -> pgp::packet::Signature {
    use pgp::crypto::{hash::HashAlgorithm, public_key::PublicKeyAlgorithm};
    use pgp::packet::{Notation, Signature};
    let mut config = SignatureConfig::v6(
        rand::thread_rng(),
        SignatureType::KeyBinding,
        PublicKeyAlgorithm::Ed25519,
        HashAlgorithm::Sha512,
    )
    .unwrap();
    let note = Notation {
        readable: true,
        name: b"pad@example.org".to_vec().into(),
        value: vec![b'x'; size].into(),
    };
    config.hashed_subpackets = vec![Subpacket::regular(SubpacketData::Notation(note)).unwrap()];
    Signature::from_config(config, [0, 0], model.signature().unwrap().clone()).unwrap()
}

/// `signature` with `embedded` in its unhashed area: what anyone can write, no key needed.
fn with_embedded(
    signature: &pgp::packet::Signature,
    embedded: pgp::packet::Signature,
) -> pgp::packet::Signature {
    let mut config = signature.config().unwrap().clone();
    config
        .unhashed_subpackets
        .push(Subpacket::regular(SubpacketData::EmbeddedSignature(Box::new(embedded))).unwrap());
    pgp::packet::Signature::from_config(
        config,
        signature.signed_hash_value().unwrap(),
        signature.signature().unwrap().clone(),
    )
    .unwrap()
}

/// The second attack on the size cap: a real key, padded through an embedded signature on its direct
/// signature, up to the limit. It verifies, because nothing checks that embedded signature.
pub fn padded_through_embedded(cert: &PublicCert, size: usize) -> PublicCert {
    use pgp::composed::{Deserializable as _, SignedPublicKey};
    let bytes = cert.to_binary().unwrap();
    let mut form = SignedPublicKey::from_bytes(std::io::Cursor::new(bytes)).unwrap();
    let signature = form.details.direct_signatures[0].clone();
    let junk = junk_embedded(&signature, size);
    form.details.direct_signatures[0] = with_embedded(&signature, junk);
    PublicCert::from_binary(&form.to_bytes().unwrap()).unwrap()
}

/// The same on the newest signature of subkey `index`.
pub fn subkey_padded_through_embedded(cert: &PublicCert, index: usize, size: usize) -> PublicCert {
    use pgp::composed::{Deserializable as _, SignedPublicKey};
    let bytes = cert.to_binary().unwrap();
    let mut form = SignedPublicKey::from_bytes(std::io::Cursor::new(bytes)).unwrap();
    let signatures = &mut form.public_subkeys[index].signatures;
    let last = signatures.pop().unwrap();
    let junk = junk_embedded(&last, size);
    signatures.push(with_embedded(&last, junk));
    PublicCert::from_binary(&form.to_bytes().unwrap()).unwrap()
}

/// `key`'s certificate with the binding of signing subkey `index` written by hand, the back-signature in
/// the unhashed area, as other programs may write it.
pub fn back_signature_unhashed(key: &SignedSecretKey, index: usize) -> PublicCert {
    use pgp::packet::KeyFlags;
    let primary = &key.primary_key;
    let mut public = key.to_public_key();
    let sub = &key.secret_subkeys[index].key;
    let back = sub
        .sign_primary_key_binding(rand::thread_rng(), primary.public_key(), &Password::empty())
        .unwrap();
    let mut config =
        SignatureConfig::from_key(rand::thread_rng(), primary, SignatureType::SubkeyBinding)
            .unwrap();
    let mut flags = KeyFlags::default();
    flags.set_sign(true);
    config.hashed_subpackets = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(Timestamp::now())).unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
        Subpacket::regular(SubpacketData::KeyFlags(flags)).unwrap(),
    ];
    config.unhashed_subpackets =
        vec![Subpacket::regular(SubpacketData::EmbeddedSignature(Box::new(back))).unwrap()];
    let binding = config
        .sign_subkey_binding(
            primary,
            primary.public_key(),
            &Password::empty(),
            &public.public_subkeys[index].key,
        )
        .unwrap();
    public.public_subkeys[index].signatures = vec![binding];
    PublicCert::from_binary(&public.to_bytes().unwrap()).unwrap()
}
