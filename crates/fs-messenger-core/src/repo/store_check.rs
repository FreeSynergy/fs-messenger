//! The refusal to open on a store without encryption (ADR 0018, 0024).
//!
//! Pattern: Specification over a real write: `Store` cannot be asked whether it is encrypted, so we
//! ask what it does: try to write a sensitive object. A plain store answers `EncryptionRequired`.
//! The check object is written once. Later checks offer it a different original, which an encrypted
//! store rejects as `OriginalImmutable` after its own encryption check, so they write nothing and
//! the log does not grow with every start. A store that opted out cannot be told apart this way.

use fs_store::{Blob, Envelope, ObjectId, Store, StoredObject};

use super::consts::{CONTENT_TYPE, SENSITIVE, STORE_CHECK};
use crate::error::{Error, Result};

const CHECK_ID: &str = "01900000-0000-7000-8000-00000000c0de";

pub(super) async fn check(store: &dyn Store, actor: &str) -> Result<()> {
    let id: ObjectId = CHECK_ID.parse()?;
    let mut env = Envelope::new(STORE_CHECK, "store check");
    env.id = id;
    env.sensitive = vec![SENSITIVE.to_string()];
    let fresh = ObjectId::new().to_string().into_bytes();
    let probe = StoredObject::new(env, CONTENT_TYPE, serde_json::json!({})).with_original(Blob {
        media_type: "text/plain".into(),
        bytes: fresh,
    });
    match store.put(probe, actor).await {
        Ok(_) | Err(fs_store::Error::OriginalImmutable(_)) => Ok(()),
        Err(fs_store::Error::EncryptionRequired(_)) => Err(Error::StoreNotEncrypted),
        Err(e) => Err(e.into()),
    }
}
