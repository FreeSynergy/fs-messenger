//! Rebuilding the memory store from the journal.
//!
//! Pattern: Decorator, part of `FsStateStore` (the type is spread over the files of its folder): opening it rebuilds
//! the wrapped memory store from the journal.
//! Free functions: journal entries back to one `StateChanges`, which the memory store takes as if
//! it had just arrived, redactions included.

use std::collections::BTreeMap;
use std::sync::Arc;

use matrix_sdk_base::StateChanges;
use matrix_sdk_base::StateStore;
use matrix_sdk_base::deserialized_responses::DisplayName;
use matrix_sdk_base::ruma::serde::Raw;
use matrix_sdk_base::ruma::{EventId, OwnedEventId};
use matrix_sdk_base::store::{MemoryStore, StateStoreDataKey, StateStoreDataValue};

use super::kind;
use super::store::FsStateStore;
use crate::error::StoreFail;
use crate::secret::SecretText;
use crate::store::Journal;
use crate::store::hex::decode_bytes;
use crate::store::keys::parts;
use crate::store::read_back::{bad, read, room, user};

fn raw<T>(text: &str) -> Result<Raw<T>, StoreFail> {
    Ok(Raw::from_json_string(text.to_owned())?)
}

fn event(text: &str) -> Result<OwnedEventId, StoreFail> {
    <&EventId>::try_from(text)
        .map(ToOwned::to_owned)
        .map_err(|e| bad("event id", e))
}

impl FsStateStore {
    /// Open over `journal`, with everything it holds.
    pub async fn open(journal: Arc<dyn Journal>) -> Result<Self, StoreFail> {
        let records = journal.load().await?;
        let memory = MemoryStore::new();
        restore(&memory, &records).await?;
        Ok(Self { memory, journal })
    }
}

async fn restore(
    memory: &MemoryStore,
    records: &BTreeMap<String, SecretText>,
) -> Result<(), StoreFail> {
    let mut c = StateChanges::default();
    let mut filters = Vec::new();
    let mut custom = Vec::new();
    let mut key_uploaded = false;

    for (key, value) in records {
        let value = value.expose();
        let p = parts(key)?;
        match (p[0].as_str(), &p[1..]) {
            (kind::SYNC_TOKEN, _) => c.sync_token = Some(read(value)?),
            (kind::ACCOUNT_DATA, [t]) => {
                c.account_data.insert(t.as_str().into(), raw(value)?);
            }
            (kind::PROFILE, [r, u]) => {
                c.profiles
                    .entry(room(r)?)
                    .or_default()
                    .insert(user(u)?, read(value)?);
            }
            (kind::NAMES, [r, name]) => {
                let names = c.ambiguity_maps.entry(room(r)?).or_default();
                names.insert(DisplayName::new(name), read(value)?);
            }
            (kind::ROOM_DATA, [r, t]) => {
                let events = c.room_account_data.entry(room(r)?).or_default();
                events.insert(t.as_str().into(), raw(value)?);
            }
            (kind::STATE, [r, t, state_key]) => {
                let types = c.state.entry(room(r)?).or_default();
                let events = types.entry(t.as_str().into()).or_default();
                events.insert(state_key.clone(), raw(value)?);
            }
            (kind::STRIPPED, [r, t, state_key]) => {
                let types = c.stripped_state.entry(room(r)?).or_default();
                let events = types.entry(t.as_str().into()).or_default();
                events.insert(state_key.clone(), raw(value)?);
            }
            (kind::ROOM_INFO, [r]) => {
                c.room_infos.insert(room(r)?, read(value)?);
            }
            (kind::REDACTION, [r, e]) => {
                c.redactions
                    .entry(room(r)?)
                    .or_default()
                    .insert(event(e)?, raw(value)?);
            }
            (kind::FILTER, [name]) => filters.push((name.clone(), read::<String>(value)?)),
            (kind::ONE_TIME_KEY, _) => key_uploaded = true,
            (kind::CUSTOM, [name]) => {
                let name = decode_bytes(name).ok_or_else(|| bad("custom key", "not hex"))?;
                let hex: String = read(value)?;
                let bytes = decode_bytes(&hex).ok_or_else(|| bad("custom value", "not hex"))?;
                custom.push((name, bytes));
            }
            (other, _) => return Err(bad("unknown record", other)),
        }
    }

    memory.save_changes(&c).await?;
    for (name, id) in filters {
        let k = StateStoreDataKey::Filter(&name);
        memory
            .set_kv_data(k, StateStoreDataValue::Filter(id))
            .await?;
    }
    if key_uploaded {
        let k = StateStoreDataKey::OneTimeKeyAlreadyUploaded;
        memory
            .set_kv_data(k, StateStoreDataValue::OneTimeKeyAlreadyUploaded)
            .await?;
    }
    for (name, bytes) in custom {
        memory.set_custom_value_no_read(&name, bytes).await?;
    }
    Ok(())
}
