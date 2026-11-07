//! [`FsKeyStore`]: the [`KeyStore`] over `fs-objects`.
//!
//! Pattern: Adapter + Repository, two paths. A SECRET key goes through `UnloggedObjects` only: no
//! activity, no event, no undo, nothing towards the hub, and so nothing that could hold it in a log or
//! print it in `Debug`. Peers are public and go through the logged `Store`. Why not a `KeyProvider`-style
//! path: that needs a key vault outside the store, which does not exist yet (ADR 0024). Every object
//! carries the sensitive category, so a plain store refuses both paths (ADR 0029). All own keys share one
//! object under a fixed id: the unlogged path has no query, and a fresh UUIDv7 would date each key.
//! Provisional object types `freesynergy.net:OwnKeys` and `freesynergy.net:PeerKey`.

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use std::collections::BTreeMap;

use fs_objects::traits::UnloggedObjects;
use fs_objects::{Envelope, ObjectId, Query, Store, StoredObject, TursoStore};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use super::{KeyStore, PeerKey};
use crate::address::normalise_address;
use crate::error::{MailError, Result};
use crate::openpgp::SecretCert;

const OWN_KEYS: &str = "freesynergy.net:OwnKeys";
/// The one object with the own keys. Fixed, and never touched by a logged call.
const OWN_KEYS_ID: &str = "00000000-0000-7000-8000-00000000f5ee";
const PEER_KEY: &str = "freesynergy.net:PeerKey";
const IDX_ADDRESS: &str = "address";
const CONTENT_TYPE: &str = "application/json";
/// Same category as the messages (`fs-messenger-core`), provisional there and here.
const SENSITIVE: &str = "pd:Communication";

/// By normalised address, the secret key in its binary form, base64.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnKeysContent {
    secrets: BTreeMap<String, String>,
}

/// Keys in a `fs-objects`. Sensitive objects: a plain store refuses every write.
pub struct FsKeyStore {
    store: Arc<dyn Store>,
    secrets: Arc<dyn UnloggedObjects>,
    actor: String,
    writes: Mutex<()>,
}

fn own_keys_id() -> Result<ObjectId> {
    OWN_KEYS_ID
        .parse()
        .map_err(|_| MailError::Keys("the id of the own keys does not parse".into()))
}

impl FsKeyStore {
    /// Over any store with the unlogged path. Behind a trait, a store told to accept sensitive objects
    /// unencrypted cannot be told apart; a program binary should use [`FsKeyStore::open_turso`].
    pub fn open<S>(store: Arc<S>, actor: impl Into<String>) -> Self
    where
        S: Store + UnloggedObjects + 'static,
    {
        Self {
            secrets: store.clone(),
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

    async fn read_secrets(&self) -> Result<OwnKeysContent> {
        match self.secrets.read_unlogged(own_keys_id()?).await? {
            Some(object) => Ok(serde_json::from_value(object.content)?),
            None => Ok(OwnKeysContent::default()),
        }
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
        let content = self.read_secrets().await?;
        let Some(secret) = content.secrets.get(&normalise_address(address)) else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(
            STANDARD
                .decode(secret)
                .map_err(|_| MailError::Keys("an own key is not base64".into()))?,
        );
        SecretCert::from_binary(&bytes).map(Some)
    }

    async fn put_own_key(&self, address: &str, key: &SecretCert) -> Result<(), MailError> {
        let _writes = self.writes.lock().await;
        let mut content = self.read_secrets().await?;
        let bytes = Zeroizing::new(key.to_binary()?);
        content.secrets.insert(
            normalise_address(address),
            STANDARD.encode(bytes.as_slice()),
        );
        let mut env = Envelope::new(OWN_KEYS, "own keys");
        env.id = own_keys_id()?;
        env.sensitive = vec![SENSITIVE.to_string()];
        let object = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(content)?);
        self.secrets.put_unlogged(object).await?;
        Ok(())
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
