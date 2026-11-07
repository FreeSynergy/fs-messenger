//! Key storage: the [`KeyStore`] trait and what it holds.

mod model;
mod store;
mod traits;

pub(crate) use model::decode_cert;
pub use model::{CandidateKey, KeySource, LegacyKey, PeerKey};
pub use store::FsKeyStore;
pub use traits::KeyStore;
