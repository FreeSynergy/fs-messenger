//! What a change of the state store looks like in the journal.
//!
//! Pattern: none, allowed by section 2 (small group of pure functions for one concept: a change of the state store as journal entries).
//! Free functions: `StateChanges` to journal entries. Raw events stay the text they were, so
//! nothing is reordered on the way. Receipts, presence and global profiles are left out: see
//! `DESIGN.md`.

use matrix_sdk_base::StateChanges;
use matrix_sdk_base::store::StateStoreDataKey;
use serde::Serialize;

use super::kind;
use crate::error::StoreFail;
use crate::store::Op;
use crate::store::keys::{key, prefix};

fn put<T: Serialize>(parts: &[&str], value: &T) -> Result<Op, StoreFail> {
    Ok(Op::Put(key(parts), serde_json::to_string(value)?.into()))
}

fn put_text(parts: &[&str], value: &str) -> Op {
    Op::Put(key(parts), value.to_owned().into())
}

/// All entries of `changes`, in the order the memory store applies them.
pub(super) fn change_ops(changes: &StateChanges) -> Result<Vec<Op>, StoreFail> {
    let mut ops = Vec::new();
    if let Some(token) = &changes.sync_token {
        ops.push(put(&[kind::SYNC_TOKEN], token)?);
    }
    for (room, users) in &changes.profiles_to_delete {
        for user in users {
            ops.push(Op::Remove(key(&[
                kind::PROFILE,
                room.as_str(),
                user.as_str(),
            ])));
        }
    }
    for (room, users) in &changes.profiles {
        for (user, profile) in users {
            ops.push(put(
                &[kind::PROFILE, room.as_str(), user.as_str()],
                profile,
            )?);
        }
    }
    for (room, names) in &changes.ambiguity_maps {
        for (name, users) in names {
            ops.push(put(
                &[kind::NAMES, room.as_str(), name.as_raw_str()],
                users,
            )?);
        }
    }
    for (event_type, raw) in &changes.account_data {
        ops.push(put_text(
            &[kind::ACCOUNT_DATA, &event_type.to_string()],
            raw.json().get(),
        ));
    }
    for (room, events) in &changes.room_account_data {
        for (event_type, raw) in events {
            let parts = [kind::ROOM_DATA, room.as_str(), &event_type.to_string()];
            ops.push(put_text(&parts, raw.json().get()));
        }
    }
    for (room, types) in &changes.state {
        // A joined room has real state; what the invite showed is gone. As in the memory store.
        if types.values().any(|events| !events.is_empty()) {
            ops.push(Op::RemovePrefix(prefix(&[kind::STRIPPED, room.as_str()])));
        }
        for (event_type, events) in types {
            for (state_key, raw) in events {
                let parts = [
                    kind::STATE,
                    room.as_str(),
                    &event_type.to_string(),
                    state_key,
                ];
                ops.push(put_text(&parts, raw.json().get()));
            }
        }
    }
    for (room, info) in &changes.room_infos {
        ops.push(put(&[kind::ROOM_INFO, room.as_str()], info)?);
    }
    for (room, types) in &changes.stripped_state {
        for (event_type, events) in types {
            for (state_key, raw) in events {
                let parts = [
                    kind::STRIPPED,
                    room.as_str(),
                    &event_type.to_string(),
                    state_key,
                ];
                ops.push(put_text(&parts, raw.json().get()));
            }
        }
    }
    for (room, redactions) in &changes.redactions {
        for (event, raw) in redactions {
            let parts = [kind::REDACTION, room.as_str(), event.as_str()];
            ops.push(put_text(&parts, raw.json().get()));
        }
    }
    Ok(ops)
}

/// The entry of a key-value record, if this kind of record is kept at all.
pub(super) fn kv_key(data: &StateStoreDataKey<'_>) -> Option<String> {
    match data {
        StateStoreDataKey::SyncToken => Some(key(&[kind::SYNC_TOKEN])),
        StateStoreDataKey::Filter(name) => Some(key(&[kind::FILTER, name])),
        StateStoreDataKey::OneTimeKeyAlreadyUploaded => Some(key(&[kind::ONE_TIME_KEY])),
        _ => None,
    }
}

/// Custom keys are bytes and journal keys text.
pub(super) fn custom_key(name: &[u8]) -> String {
    key(&[kind::CUSTOM, &crate::store::hex::encode_bytes(name)])
}

/// All that hangs on a room. Its info is one record, the rest hang below the room's id.
pub(super) fn room_ops(room: &str) -> Vec<Op> {
    let below = kind::OF_ROOM
        .iter()
        .map(|k| Op::RemovePrefix(prefix(&[k, room])));
    let mut ops: Vec<Op> = below.collect();
    ops.push(Op::Remove(key(&[kind::ROOM_INFO, room])));
    ops
}
