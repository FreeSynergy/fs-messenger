//! Rebuilding the memory store from the journal.
//!
//! Pattern: Adapter: journal entries back to `Changes`, which the memory store then takes as if they
//! had just happened. An unreadable entry fails the opening: a crypto store with holes would make new
//! keys and lose the old identity without saying so.

use std::collections::BTreeMap;
use std::sync::Arc;

use matrix_sdk::ruma::events::secret::request::SecretName;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use matrix_sdk_base::crypto::olm::{
    Account, InboundGroupSession, OutboundGroupSession, PickledAccount,
    PickledCrossSigningIdentity, PickledInboundGroupSession, PickledOutboundGroupSession,
    PickledSession, PrivateCrossSigningIdentity, Session, StaticAccountData,
};
use matrix_sdk_base::crypto::store::CryptoStore;
use matrix_sdk_base::crypto::store::MemoryStore;
use matrix_sdk_base::crypto::store::types::{Changes, PendingChanges, SecretsInboxItem};
use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

use super::decode_bytes;
use super::{FsCryptoStore, kind};
use crate::error::StoreFail;
use crate::store::Journal;
use crate::store::keys::parts;

fn read<T: DeserializeOwned>(text: &str) -> Result<T, StoreFail> {
    Ok(serde_json::from_str(text)?)
}

fn bad(what: &str, why: impl std::fmt::Display) -> StoreFail {
    StoreFail::Record(format!("{what}: {why}"))
}

fn room(text: &str) -> Result<OwnedRoomId, StoreFail> {
    <&RoomId>::try_from(text)
        .map(ToOwned::to_owned)
        .map_err(|e| bad("room id", e))
}

fn user(text: &str) -> Result<OwnedUserId, StoreFail> {
    <&UserId>::try_from(text)
        .map(ToOwned::to_owned)
        .map_err(|e| bad("user id", e))
}

impl FsCryptoStore {
    /// Open over `journal`, with everything it holds.
    pub async fn open(journal: Arc<dyn Journal>) -> Result<Self, StoreFail> {
        let records = journal.load().await?;
        let memory = MemoryStore::new();
        let (identity, static_account) = restore(&memory, &records).await?;
        Ok(Self {
            memory,
            journal,
            identity: RwLock::new(identity),
            static_account: std::sync::RwLock::new(static_account),
            saving: Mutex::new(()),
        })
    }
}

async fn restore(
    memory: &MemoryStore,
    records: &BTreeMap<String, String>,
) -> Result<
    (
        Option<PrivateCrossSigningIdentity>,
        Option<StaticAccountData>,
    ),
    StoreFail,
> {
    let mut static_account = None;
    let mut changes = Changes::default();
    let mut identity = None;
    let mut account = None;
    let mut sessions = Vec::new();
    let mut outbound = Vec::new();
    let mut backed_up = Vec::new();
    let mut tracked = Vec::new();
    let mut custom = Vec::new();

    for (key, value) in records {
        let p = parts(key)?;
        match (p[0].as_str(), &p[1..]) {
            (kind::ACCOUNT, _) => account = Some(read::<PickledAccount>(value)?),
            (kind::IDENTITY, _) => {
                let pickle: PickledCrossSigningIdentity = read(value)?;
                identity = Some(
                    PrivateCrossSigningIdentity::from_pickle(pickle)
                        .map_err(|e| bad("identity", e))?,
                );
            }
            (kind::BACKUP_VERSION, _) => changes.backup_version = Some(read(value)?),
            (kind::BACKUP_KEY, _) => changes.backup_decryption_key = Some(read(value)?),
            (kind::DEHYDRATED, _) => changes.dehydrated_device_pickle_key = Some(read(value)?),
            (kind::SESSION, _) => sessions.push(read::<PickledSession>(value)?),
            (kind::HASH, _) => changes.message_hashes.push(read(value)?),
            (kind::INBOUND, _) => {
                let pickle: PickledInboundGroupSession = read(value)?;
                changes
                    .inbound_group_sessions
                    .push(InboundGroupSession::from_pickle(pickle).map_err(|e| bad("inbound", e))?);
            }
            (kind::INBOUND_BACKED_UP, [r, s]) => {
                backed_up.push((room(r)?, s.clone(), read::<String>(value)?));
            }
            (kind::OUTBOUND, _) => outbound.push(read::<PickledOutboundGroupSession>(value)?),
            (kind::REQUEST, _) => changes.key_requests.push(read(value)?),
            (kind::USER_IDENTITY, _) => changes.identities.new.push(read(value)?),
            (kind::DEVICE, _) => changes.devices.new.push(read(value)?),
            (kind::WITHHELD, [r, s]) => {
                let entries = changes.withheld_session_info.entry(room(r)?).or_default();
                entries.insert(s.clone(), read(value)?);
            }
            (kind::SETTINGS, [r]) => {
                changes.room_settings.insert(room(r)?, read(value)?);
            }
            (kind::SECRET, [name, _]) => changes.secrets.push(SecretsInboxItem {
                secret_name: SecretName::from(name.as_str()),
                secret: Zeroizing::new(read(value)?),
            }),
            (kind::NEXT_BATCH, _) => changes.next_batch_token = Some(read(value)?),
            (kind::BUNDLE, _) => changes.received_room_key_bundles.push(read(value)?),
            (kind::DOWNLOADED, [r]) => {
                changes.room_key_backups_fully_downloaded.insert(room(r)?);
            }
            (kind::PENDING_BUNDLE, [r]) => {
                changes
                    .rooms_pending_key_bundle
                    .insert(room(r)?, Some(read(value)?));
            }
            (kind::TRACKED, [u]) => tracked.push((user(u)?, read::<bool>(value)?)),
            (kind::CUSTOM, [name]) => {
                let hex: String = read(value)?;
                let bytes = decode_bytes(&hex).ok_or_else(|| bad("custom value", "not hex"))?;
                custom.push((name.clone(), bytes));
            }
            (other, _) => return Err(bad("unknown record", other)),
        }
    }

    // Sessions and outbound keys need the account's own keys.
    if let Some(pickle) = account {
        let account = Account::from_pickle(pickle).map_err(|e| bad("account", e))?;
        static_account = Some(account.static_data().clone());
        let device_keys = account.device_keys();
        let device_id = account.device_id().to_owned();
        let identity_keys = account.static_data().identity_keys.clone();
        for pickle in sessions {
            let session =
                Session::from_pickle(device_keys.clone(), pickle).map_err(|e| bad("session", e))?;
            changes.sessions.push(session);
        }
        for pickle in outbound {
            let session =
                OutboundGroupSession::from_pickle(device_id.clone(), identity_keys.clone(), pickle)
                    .map_err(|e| bad("outbound", e))?;
            changes.outbound_group_sessions.push(session);
        }
        let pending = PendingChanges {
            account: Some(account),
        };
        memory.save_pending_changes(pending).await?;
    } else if !sessions.is_empty() || !outbound.is_empty() {
        return Err(bad("sessions", "no account in the journal"));
    }
    memory.save_changes(changes).await?;

    for (r, s, version) in backed_up {
        let ids = [(r.as_ref(), s.as_str())];
        memory
            .mark_inbound_group_sessions_as_backed_up(&version, &ids)
            .await?;
    }
    let tracked: Vec<(&UserId, bool)> = tracked.iter().map(|(u, d)| (u.as_ref(), *d)).collect();
    memory.save_tracked_users(&tracked).await?;
    for (name, bytes) in custom {
        memory.set_custom_value(&name, bytes).await?;
    }
    Ok((identity, static_account))
}
