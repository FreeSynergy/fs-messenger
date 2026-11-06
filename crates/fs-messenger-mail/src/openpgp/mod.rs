//! OpenPGP with rPGP (`pgp`), never sequoia (ADR 0018).

mod cert;
mod decrypt;
mod encrypt;
mod generate;
mod usable;

pub use cert::{PublicCert, SecretCert};
pub use decrypt::{SignatureState, decrypt, decrypt_selecting, verify_detached};
pub use encrypt::{EncryptionTargets, sign_and_encrypt};
pub use generate::generate_key;
pub(crate) use usable::may_vouch;
pub use usable::{KeyRefusal, unix_now, usable_encryption_key};

#[cfg(test)]
mod roundtrip_tests;
#[cfg(test)]
mod usable_tests;
