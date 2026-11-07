//! Device signatures: [`DeviceVerifier`], and Ed25519 behind it.
//!
//! Pattern: Strategy: the guard depends on the trait; the key type is provisional (contract). The
//! implementation is `ed25519-compact` (pure Rust, MIT). `ed25519-dalek` was not taken, as it and
//! `curve25519-dalek` are BSD-3-Clause. Private device keys are never held here: there is no type for
//! them in this crate outside the tests.

use ed25519_compact::{PublicKey, Signature};

/// Checks a device signature. Only public keys come in.
pub trait DeviceVerifier: Send + Sync {
    /// The name of the key type, stored with each key.
    fn key_type(&self) -> &'static str;
    /// Whether this is a usable public key.
    fn accepts_key(&self, public_key: &[u8]) -> bool;
    /// Whether `signature` by `public_key` fits `message` exactly.
    fn verify(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> bool;
}

/// Ed25519 (RFC 8032). Refuses malformed keys, a small-order or identity key, and a non-canonical `s`.
/// A signature may have other bytes for the same message (a small-order part in `R`), so single use
/// goes by the signed bytes and never by the signature's.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ed25519Verifier;

/// The base point, with `s` = 0: a well-formed signature, so that opening a verification can only fail
/// on the key. The library refuses a small-order or identity key at this point, and has no check of
/// its own for a key alone.
const PROBE: [u8; 64] = {
    let mut probe = [0x66u8; 64];
    probe[0] = 0x58;
    let mut at = 32;
    while at < 64 {
        probe[at] = 0;
        at += 1;
    }
    probe
};

impl DeviceVerifier for Ed25519Verifier {
    fn key_type(&self) -> &'static str {
        "ed25519"
    }

    fn accepts_key(&self, public_key: &[u8]) -> bool {
        PublicKey::from_slice(public_key)
            .is_ok_and(|key| key.verify_incremental(&Signature::new(PROBE)).is_ok())
    }

    fn verify(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        let (Ok(key), Ok(signature)) = (
            PublicKey::from_slice(public_key),
            Signature::from_slice(signature),
        ) else {
            return false;
        };
        key.verify(message, &signature).is_ok()
    }
}
