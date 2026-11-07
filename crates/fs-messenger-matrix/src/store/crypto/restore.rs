//! Rebuilding the memory store from the journal.
//!
//! Pattern: Decorator, part of FsCryptoStore (the type is spread over the files of its folder): opening it rebuilds
//! the wrapped memory store from the journal.
//! Free functions: journal entries back to `Changes`, which the memory store then takes as if they
//! had just happened. An unreadable entry fails the opening: a crypto store with holes would make
//! new keys and lose the old identity without saying so.

use std::collections::BTreeMap;
use std::mem::take;
use std::sync::Arc;

use matrix_sdk::ruma::events::secret::request::SecretName;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk_base::crypto::olm::{
    Account, InboundGroupSession, OutboundGroupSession, PickledAccount,
    PickledCrossSigningIdentity, PickledInboundGroupSession, PickledOutboundGroupSession,
    PickledSession, PrivateCrossSigningIdentity, Session, StaticAccountData,
};
use matrix_sdk_base::crypto::store::CryptoStore;
use matrix_sdk_base::crypto::store::MemoryStore;
use matrix_sdk_base::crypto::store::types::{Changes, PendingChanges, SecretsInboxItem};
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

use super::kind;
use super::store::FsCryptoStore;
use crate::error::StoreFail;
use crate::secret::SecretText;
use crate::store::Journal;
use crate::store::hex::decode_bytes;
use crate::store::keys::parts;
use crate::store::read_back::{bad, read, room, unreadable, user};

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

/// What the journal holds, sorted out; some of it needs the account before it can become objects.
#[derive(Default)]
struct Collected {
    changes: Changes,
    identity: Option<PrivateCrossSigningIdentity>,
    account: Option<PickledAccount>,
    sessions: Vec<PickledSession>,
    outbound: Vec<PickledOutboundGroupSession>,
    backed_up: Vec<(OwnedRoomId, String, String)>,
    tracked: Vec<(OwnedUserId, bool)>,
    custom: Vec<(String, Vec<u8>)>,
}

impl Collected {
    fn take(&mut self, key: &str, value: &str) -> Result<(), StoreFail> {
        let p = parts(key)?;
        let changes = &mut self.changes;
        match (p[0].as_str(), &p[1..]) {
            (kind::ACCOUNT, _) => self.account = Some(read(value)?),
            (kind::IDENTITY, _) => {
                let pickle: PickledCrossSigningIdentity = read(value)?;
                let identity = PrivateCrossSigningIdentity::from_pickle(pickle);
                self.identity = Some(identity.map_err(|_| unreadable("identity"))?);
            }
            (kind::BACKUP_VERSION, _) => changes.backup_version = Some(read(value)?),
            (kind::BACKUP_KEY, _) => changes.backup_decryption_key = Some(read(value)?),
            (kind::DEHYDRATED, _) => changes.dehydrated_device_pickle_key = Some(read(value)?),
            (kind::SESSION, _) => self.sessions.push(read(value)?),
            (kind::HASH, _) => changes.message_hashes.push(read(value)?),
            (kind::INBOUND, _) => {
                let pickle: PickledInboundGroupSession = read(value)?;
                let session = InboundGroupSession::from_pickle(pickle);
                let session = session.map_err(|_| unreadable("inbound session"))?;
                changes.inbound_group_sessions.push(session);
            }
            (kind::INBOUND_BACKED_UP, [r, s]) => {
                self.backed_up.push((room(r)?, s.clone(), read(value)?));
            }
            (kind::OUTBOUND, _) => self.outbound.push(read(value)?),
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
                let details = Some(read(value)?);
                changes.rooms_pending_key_bundle.insert(room(r)?, details);
            }
            (kind::TRACKED, [u]) => self.tracked.push((user(u)?, read(value)?)),
            (kind::CUSTOM, [name]) => {
                let hex: String = read(value)?;
                let bytes = decode_bytes(&hex).ok_or_else(|| bad("custom value", "not hex"))?;
                self.custom.push((name.clone(), bytes));
            }
            (other, _) => return Err(bad("unknown record", other)),
        }
        Ok(())
    }

    /// Sessions and outbound keys need the account's own keys, so it goes first.
    async fn account_into(
        &mut self,
        memory: &MemoryStore,
    ) -> Result<Option<StaticAccountData>, StoreFail> {
        let (sessions, outbound) = (take(&mut self.sessions), take(&mut self.outbound));
        let Some(pickle) = self.account.take() else {
            if sessions.is_empty() && outbound.is_empty() {
                return Ok(None);
            }
            return Err(bad("sessions", "no account in the journal"));
        };
        let account = Account::from_pickle(pickle).map_err(|_| unreadable("account"))?;
        let data = account.static_data().clone();
        let device_keys = account.device_keys();
        let device_id = account.device_id().to_owned();
        for pickle in sessions {
            let session = Session::from_pickle(device_keys.clone(), pickle);
            self.changes
                .sessions
                .push(session.map_err(|_| unreadable("session"))?);
        }
        for pickle in outbound {
            let keys = data.identity_keys.clone();
            let session = OutboundGroupSession::from_pickle(device_id.clone(), keys, pickle);
            self.changes
                .outbound_group_sessions
                .push(session.map_err(|_| unreadable("outbound session"))?);
        }
        let pending = PendingChanges {
            account: Some(account),
        };
        memory.save_pending_changes(pending).await?;
        Ok(Some(data))
    }
}

type Restored = (
    Option<PrivateCrossSigningIdentity>,
    Option<StaticAccountData>,
);

async fn restore(
    memory: &MemoryStore,
    records: &BTreeMap<String, SecretText>,
) -> Result<Restored, StoreFail> {
    let mut c = Collected::default();
    for (key, value) in records {
        c.take(key, value.expose())?;
    }
    let static_account = c.account_into(memory).await?;
    memory.save_changes(c.changes).await?;
    for (r, s, version) in c.backed_up {
        let ids = [(r.as_ref(), s.as_str())];
        memory
            .mark_inbound_group_sessions_as_backed_up(&version, &ids)
            .await?;
    }
    let tracked: Vec<(&UserId, bool)> = c.tracked.iter().map(|(u, d)| (u.as_ref(), *d)).collect();
    memory.save_tracked_users(&tracked).await?;
    for (name, bytes) in c.custom {
        memory.set_custom_value(&name, bytes).await?;
    }
    Ok((c.identity, static_account))
}
