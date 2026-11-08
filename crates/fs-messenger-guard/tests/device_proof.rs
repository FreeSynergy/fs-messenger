//! The device proof: Ed25519, exact signed bytes, keys that are not, single use, enrolment.

mod common;

use common::{Device, OWNER};
use fs_messenger_guard::{CONTEXT, DeviceVerifier, Ed25519Verifier, Method, signed_bytes};
use std::fmt::Write as _;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn rfc_8032_test_vector_2() {
    let key = unhex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
    let message = unhex("72");
    let signature = unhex(
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
         085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    );
    let v = Ed25519Verifier;
    assert!(v.accepts_key(&key));
    assert!(v.verify(&key, &message, &signature));
    assert!(!v.verify(&key, &unhex("73"), &signature));
}

#[test]
fn a_key_of_low_order_a_malformed_key_and_a_non_canonical_s_are_refused() {
    let v = Ed25519Verifier;
    let identity = unhex("0100000000000000000000000000000000000000000000000000000000000000");
    let order_two = unhex("ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
    let zero = vec![0u8; 32];
    for key in [&identity, &order_two, &zero] {
        assert!(!v.accepts_key(key), "{}", hex(key));
        // The signature that would pass for every message under such a key.
        let mut any = identity.clone();
        any.extend_from_slice(&[0u8; 32]);
        assert!(!v.verify(key, b"anything", &any));
    }
    assert!(!v.accepts_key(&[1, 2, 3]));
    assert!(!v.accepts_key(&[]));

    let d = Device::new(OWNER, 1);
    let good = d.sign(b"m");
    assert!(v.verify(&d.public(), b"m", &good));
    // s + L: the same point arithmetic, and not canonical.
    let l = unhex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    let mut bad = good.clone();
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(bad[32 + i]) + u16::from(l[i]) + carry;
        bad[32 + i] = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    assert!(!v.verify(&d.public(), b"m", &bad));
    assert!(!v.verify(&d.public(), b"m", &good[..63]));
    assert!(!v.verify(&d.public(), b"m", &[]));
}

#[test]
fn the_signed_bytes_are_exactly_these() {
    assert_eq!(CONTEXT, "FreeSynergy.Messenger guard device proof v1");
    let bytes = signed_bytes(
        &[0xee; 2],
        Method::Release,
        "me",
        &[b"ai", b"changes"],
        b"nn",
    )
    .unwrap();
    let mut expected = vec![];
    let field = |out: &mut Vec<u8>, f: &[u8]| {
        out.extend_from_slice(&u32::try_from(f.len()).unwrap().to_be_bytes());
        out.extend_from_slice(f);
    };
    field(&mut expected, CONTEXT.as_bytes());
    field(&mut expected, &[0xee, 0xee]);
    field(&mut expected, b"release");
    field(&mut expected, b"me");
    expected.extend_from_slice(&2u32.to_be_bytes());
    field(&mut expected, b"ai");
    field(&mut expected, b"changes");
    field(&mut expected, b"nn");
    assert_eq!(bytes, expected);
    // A fixed vector too, so that the helper above cannot drift along with the code.
    assert_eq!(
        hex(&signed_bytes(b"E", Method::Stop, "s", &[], b"N").unwrap()),
        concat!(
            "0000002b",
            "46726565 53796e65 7267792e 4d657373 656e6765 72206775 61726420 64657669 63652070 726f6f66 2076 31",
            "00000001", "45",
            "00000004", "73746f70",
            "00000001", "73",
            "00000000",
            "00000001", "4e",
        )
        .replace(' ', "")
    );
}

#[test]
fn fields_cannot_be_shifted_into_each_other() {
    let sign = |engine: &[u8], signer: &str, arguments: &[&[u8]], nonce: &[u8]| {
        signed_bytes(engine, Method::Release, signer, arguments, nonce).unwrap()
    };
    assert_ne!(
        sign(b"e", "s", &[b"ab", b"c"], b"n"),
        sign(b"e", "s", &[b"a", b"bc"], b"n")
    );
    assert_ne!(
        sign(b"e", "s", &[b"ab"], b"c"),
        sign(b"e", "s", &[b"a"], b"bc")
    );
    assert_ne!(sign(b"e", "sa", &[], b"n"), sign(b"es", "a", &[], b"n"));
    assert_ne!(sign(b"e", "s", &[b""], b"n"), sign(b"e", "s", &[], b"n"));
    assert_ne!(
        sign(b"e", "s", &[b"", b"a"], b"n"),
        sign(b"e", "s", &[b"a", b""], b"n")
    );
}
