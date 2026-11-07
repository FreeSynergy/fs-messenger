//! OpenPGP with rPGP (`pgp`), never sequoia (ADR 0018).

mod cert;
mod decrypt;
mod encrypt;
mod generate;
mod merge;
mod shape;
mod usable;

pub(crate) use cert::MAX_CERT_BYTES;
pub use cert::{PublicCert, SecretCert};
pub use decrypt::{SignatureState, decrypt, decrypt_selecting, verify_detached};
pub use encrypt::{EncryptionTargets, Method, sign_and_encrypt};
pub use generate::{generate_key, generate_legacy_key};
pub use shape::{Shape, method_of};
pub(crate) use usable::may_vouch;
pub use usable::{KeyRefusal, supports_seipd_v2, unix_now, usable_encryption_key};

#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod roundtrip_tests;
#[cfg(test)]
mod subkey_tests;
#[cfg(test)]
mod usable_tests;
