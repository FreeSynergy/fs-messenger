//! What a change of the crypto store looks like in the journal.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points).
//! Free functions: `Changes` to journal entries. Pickles of the SDK are serde values, and the
//! pickle key is none: the store around them is what is encrypted (ADR 0024).

use matrix_sdk_base::crypto::olm::InboundGroupSession;
use matrix_sdk_base::crypto::store::types::Changes;
use serde::Serialize;

use super::kind;
use crate::error::StoreFail;
use crate::store::Op;
use crate::store::hex::encode_bytes;
use crate::store::keys::key;

fn put<T: Serialize>(parts: &[&str], value: &T) -> Result<Op, StoreFail> {
    Ok(Op::Put(key(parts), serde_json::to_string(value)?.into()))
}

/// The entry of one inbound Megolm session, and of the backup it is in, if any.
pub(super) async fn inbound_ops(
    session: &InboundGroupSession,
    backed_up: Option<&str>,
) -> Result<Vec<Op>, StoreFail> {
    let room = session.room_id().as_str();
    let id = session.session_id();
    let mut ops = vec![put(&[kind::INBOUND, room, id], &session.pickle().await)?];
    if let Some(version) = backed_up {
        ops.push(put(&[kind::INBOUND_BACKED_UP, room, id], &version)?);
    }
    Ok(ops)
}

/// All entries of `changes`. The identity is left in `changes` for the caller to keep.
pub(super) async fn change_ops(changes: &Changes) -> Result<Vec<Op>, StoreFail> {
    let mut ops = Vec::new();
    if let Some(identity) = &changes.private_identity {
        ops.push(put(&[kind::IDENTITY], &identity.pickle().await)?);
    }
    if let Some(version) = &changes.backup_version {
        ops.push(put(&[kind::BACKUP_VERSION], version)?);
    }
    if let Some(backup_key) = &changes.backup_decryption_key {
        ops.push(put(&[kind::BACKUP_KEY], backup_key)?);
    }
    if let Some(device_key) = &changes.dehydrated_device_pickle_key {
        ops.push(put(&[kind::DEHYDRATED], device_key)?);
    }
    for session in &changes.sessions {
        let pickle = session.pickle().await;
        let sender = pickle.sender_key.to_base64();
        ops.push(put(
            &[kind::SESSION, &sender, session.session_id()],
            &pickle,
        )?);
    }
    for hash in &changes.message_hashes {
        ops.push(put(&[kind::HASH, &hash.sender_key, &hash.hash], hash)?);
    }
    for session in &changes.inbound_group_sessions {
        ops.extend(inbound_ops(session, None).await?);
    }
    for session in &changes.outbound_group_sessions {
        let parts = [kind::OUTBOUND, session.room_id().as_str()];
        ops.push(put(&parts, &session.pickle().await)?);
    }
    for request in &changes.key_requests {
        ops.push(put(&[kind::REQUEST, request.request_id.as_str()], request)?);
    }
    for identity in changes
        .identities
        .new
        .iter()
        .chain(&changes.identities.changed)
    {
        ops.push(put(
            &[kind::USER_IDENTITY, identity.user_id().as_str()],
            identity,
        )?);
    }
    for device in changes.devices.new.iter().chain(&changes.devices.changed) {
        let parts = [
            kind::DEVICE,
            device.user_id().as_str(),
            device.device_id().as_str(),
        ];
        ops.push(put(&parts, device)?);
    }
    for device in &changes.devices.deleted {
        let parts = [
            kind::DEVICE,
            device.user_id().as_str(),
            device.device_id().as_str(),
        ];
        ops.push(Op::Remove(key(&parts)));
    }
    for (room, sessions) in &changes.withheld_session_info {
        for (session, entry) in sessions {
            ops.push(put(&[kind::WITHHELD, room.as_str(), session], entry)?);
        }
    }
    for (room, settings) in &changes.room_settings {
        ops.push(put(&[kind::SETTINGS, room.as_str()], settings)?);
    }
    for secret in &changes.secrets {
        let name = secret.secret_name.to_string();
        let unique = fs_store::ObjectId::new().to_string();
        ops.push(put(&[kind::SECRET, &name, &unique], &*secret.secret)?);
    }
    if let Some(token) = &changes.next_batch_token {
        ops.push(put(&[kind::NEXT_BATCH], token)?);
    }
    for bundle in &changes.received_room_key_bundles {
        let parts = [
            kind::BUNDLE,
            bundle.bundle_data.room_id.as_str(),
            bundle.sender_user.as_str(),
        ];
        ops.push(put(&parts, bundle)?);
    }
    for room in &changes.room_key_backups_fully_downloaded {
        ops.push(put(&[kind::DOWNLOADED, room.as_str()], &true)?);
    }
    for (room, details) in &changes.rooms_pending_key_bundle {
        let parts = [kind::PENDING_BUNDLE, room.as_str()];
        ops.push(match details {
            Some(details) => put(&parts, details)?,
            None => Op::Remove(key(&parts)),
        });
    }
    Ok(ops)
}

/// The entry of a custom value.
pub(super) fn custom_op(name: &str, value: &[u8]) -> Result<Op, StoreFail> {
    put(&[kind::CUSTOM, name], &encode_bytes(value))
}
