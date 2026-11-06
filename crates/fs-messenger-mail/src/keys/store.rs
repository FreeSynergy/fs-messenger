//! [`FsKeyStore`]: the [`KeyStore`] over `fs-store`.
//!
//! Pattern: Adapter + Repository. Every object carries the sensitive category, so `fs-store` refuses
//! it on a plain store (ADR 0029). Provisional object types `freesynergy.net:OwnKey` and
//! `freesynergy.net:PeerKey`, keyed by the index field `address`. A store that synchronises to a hub
//! must not hold these: give the keys a store of their own (`DESIGN.md`).

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fs_store::{Envelope, Query, Store, StoredObject, TursoStore};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use super::{KeyStore, PeerKey};
use crate::address::normalise_address;
use crate::error::{MailError, Result};
use crate::openpgp::SecretCert;

const OWN_KEY: &str = "freesynergy.net:OwnKey";
const PEER_KEY: &str = "freesynergy.net:PeerKey";
const IDX_ADDRESS: &str = "address";
const CONTENT_TYPE: &str = "application/json";
/// Same category as the messages (`fs-messenger-core`), provisional there and here.
const SENSITIVE: &str = "pd:Communication";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnKeyContent {
    secret: String,
}

/// Keys in a `fs-store`. Sensitive objects: a plain store refuses every write.
pub struct FsKeyStore {
    store: Arc<dyn Store>,
    actor: String,
    writes: Mutex<()>,
}

impl FsKeyStore {
    /// Over any store. Behind `dyn Store`, a store told to accept sensitive objects unencrypted cannot
    /// be told apart; a program binary should use [`FsKeyStore::open_turso`].
    pub fn open(store: Arc<dyn Store>, actor: impl Into<String>) -> Self {
        Self {
            store,
            actor: actor.into(),
            writes: Mutex::new(()),
        }
    }

    /// Over a Turso store, which can be asked: refuses every unencrypted one.
    pub fn open_turso(store: Arc<TursoStore>, actor: impl Into<String>) -> Result<Self, MailError> {
        if !store.is_encrypted() {
            return Err(MailError::Keys(
                "the store is not encrypted; keys do not go there (ADR 0024)".into(),
            ));
        }
        Ok(Self::open(store, actor))
    }

    async fn find(&self, kind: &str, address: &str) -> Result<Option<StoredObject>> {
        let q = Query::new()
            .of_type(kind)
            .with_index(IDX_ADDRESS, normalise_address(address))
            .limit(1);
        Ok(self.store.query(&q).await?.pop())
    }

    /// Write `content` under the object for `address`, in place if there is one.
    async fn upsert(&self, kind: &str, address: &str, content: serde_json::Value) -> Result<()> {
        let address = normalise_address(address);
        let _writes = self.writes.lock().await;
        let mut env = Envelope::new(kind, address.clone());
        if let Some(old) = self.find(kind, &address).await? {
            env.id = old.envelope.id;
        }
        env.sensitive = vec![SENSITIVE.to_string()];
        let object = StoredObject::new(env, CONTENT_TYPE, content).with_index(IDX_ADDRESS, address);
        self.store.put(object, &self.actor).await?;
        Ok(())
    }
}

#[async_trait]
impl KeyStore for FsKeyStore {
    async fn own_key(&self, address: &str) -> Result<Option<SecretCert>, MailError> {
        let Some(object) = self.find(OWN_KEY, address).await? else {
            return Ok(None);
        };
        let content: OwnKeyContent = serde_json::from_value(object.content)?;
        let bytes = Zeroizing::new(
            STANDARD
                .decode(&content.secret)
                .map_err(|e| MailError::Keys(e.to_string()))?,
        );
        SecretCert::from_binary(&bytes).map(Some)
    }

    async fn put_own_key(&self, address: &str, key: &SecretCert) -> Result<(), MailError> {
        let bytes = Zeroizing::new(key.to_binary()?);
        let content = serde_json::to_value(OwnKeyContent {
            secret: STANDARD.encode(bytes.as_slice()),
        })?;
        self.upsert(OWN_KEY, address, content).await
    }

    async fn peer(&self, address: &str) -> Result<Option<PeerKey>, MailError> {
        let Some(object) = self.find(PEER_KEY, address).await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(object.content)?))
    }

    async fn put_peer(&self, peer: &PeerKey) -> Result<(), MailError> {
        self.upsert(PEER_KEY, &peer.address, serde_json::to_value(peer)?)
            .await
    }
}
