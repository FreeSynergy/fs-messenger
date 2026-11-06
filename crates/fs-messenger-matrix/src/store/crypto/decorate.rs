//! The `CryptoStore` trait over the memory store and the journal.
//!
//! Pattern: Decorator: every write goes to the journal first; reads pass on.

use std::collections::HashMap;

use matrix_sdk::cross_process_lock::CrossProcessLockGeneration;
use matrix_sdk::ruma::events::secret::request::SecretName;
use matrix_sdk::ruma::{DeviceId, OwnedDeviceId, RoomId, TransactionId, UserId};
use matrix_sdk_base::crypto::olm::{
    Account, InboundGroupSession, OutboundGroupSession, PrivateCrossSigningIdentity,
    SenderDataType, Session,
};
use matrix_sdk_base::crypto::store::CryptoStore;
use matrix_sdk_base::crypto::store::types::{
    BackupKeys, Changes, DehydratedDeviceKey, PendingChanges, RoomKeyCounts, RoomKeyWithheldEntry,
    RoomPendingKeyBundleDetails, RoomSettings, StoredRoomKeyBundleData, TrackedUser,
};
use matrix_sdk_base::crypto::vodozemac::Curve25519PublicKey;
use matrix_sdk_base::crypto::{DeviceData, GossipRequest, SecretInfo, UserIdentityData};
use zeroize::Zeroizing;

use super::persist::{change_ops, custom_op, inbound_ops};
use super::{FsCryptoStore, kind};
use crate::error::StoreFail;
use crate::store::Op;
use crate::store::forward::decorate_memory;
use crate::store::keys::{key, prefix};

decorate_memory! {
    impl CryptoStore, for FsCryptoStore, error StoreFail;
    explicit {
        async fn load_identity(&self) -> Result<Option<PrivateCrossSigningIdentity>, StoreFail> {
            Ok(self.identity.read().await.clone())
        }

        async fn save_pending_changes(&self, changes: PendingChanges) -> Result<(), StoreFail> {
            let Some(account) = changes.account else {
                // The memory store would forget the account on this.
                return Ok(());
            };
            let _saving = self.saving.lock().await;
            self.remember_account(account.static_data().clone());
            let pickle = serde_json::to_string(&account.pickle())?;
            self.journal.append(vec![Op::Put(key(&[kind::ACCOUNT]), pickle)]).await?;
            let pending = PendingChanges { account: Some(account) };
            Ok(self.memory.save_pending_changes(pending).await?)
        }

        async fn save_changes(&self, mut changes: Changes) -> Result<(), StoreFail> {
            let _saving = self.saving.lock().await;
            self.journal.append(change_ops(&changes).await?).await?;
            if let Some(identity) = changes.private_identity.take() {
                *self.identity.write().await = Some(identity);
            }
            Ok(self.memory.save_changes(changes).await?)
        }

        async fn save_inbound_group_sessions(
            &self,
            sessions: Vec<InboundGroupSession>,
            backed_up_to_version: Option<&str>,
        ) -> Result<(), StoreFail> {
            let _saving = self.saving.lock().await;
            let mut ops = Vec::with_capacity(sessions.len());
            for session in &sessions {
                ops.extend(inbound_ops(session, backed_up_to_version).await?);
            }
            self.journal.append(ops).await?;
            Ok(self.memory.save_inbound_group_sessions(sessions, backed_up_to_version).await?)
        }

        async fn mark_inbound_group_sessions_as_backed_up(
            &self,
            backup_version: &str,
            room_and_session_ids: &[(&RoomId, &str)],
        ) -> Result<(), StoreFail> {
            let _saving = self.saving.lock().await;
            let version = serde_json::to_string(backup_version)?;
            let mut ops = Vec::new();
            for (room, session) in room_and_session_ids {
                let k = key(&[kind::INBOUND_BACKED_UP, room.as_str(), session]);
                ops.push(Op::Put(k, version.clone()));
                // The session's own flag, in the pickle, has to agree after a restart.
                if let Some(s) = self.memory.get_inbound_group_session(room, session).await? {
                    s.mark_as_backed_up();
                    ops.extend(inbound_ops(&s, None).await?);
                }
            }
            self.journal.append(ops).await?;
            Ok(self
                .memory
                .mark_inbound_group_sessions_as_backed_up(backup_version, room_and_session_ids)
                .await?)
        }

        async fn reset_backup_state(&self) -> Result<(), StoreFail> {
            let _saving = self.saving.lock().await;
            let ops = vec![Op::RemovePrefix(prefix(&[kind::INBOUND_BACKED_UP]))];
            self.journal.append(ops).await?;
            Ok(self.memory.reset_backup_state().await?)
        }

        async fn delete_dehydrated_device_pickle_key(&self) -> Result<(), StoreFail> {
            self.journal.append(vec![Op::Remove(key(&[kind::DEHYDRATED]))]).await?;
            Ok(self.memory.delete_dehydrated_device_pickle_key().await?)
        }

        async fn save_tracked_users(&self, users: &[(&UserId, bool)]) -> Result<(), StoreFail> {
            let _saving = self.saving.lock().await;
            let mut ops = Vec::with_capacity(users.len());
            for (user, dirty) in users {
                let value = serde_json::to_string(dirty)?;
                ops.push(Op::Put(key(&[kind::TRACKED, user.as_str()]), value));
            }
            self.journal.append(ops).await?;
            Ok(self.memory.save_tracked_users(users).await?)
        }

        async fn delete_outgoing_secret_requests(
            &self,
            request_id: &TransactionId,
        ) -> Result<(), StoreFail> {
            let k = key(&[kind::REQUEST, request_id.as_str()]);
            self.journal.append(vec![Op::Remove(k)]).await?;
            Ok(self.memory.delete_outgoing_secret_requests(request_id).await?)
        }

        async fn delete_secrets_from_inbox(
            &self,
            secret_name: &SecretName,
        ) -> Result<(), StoreFail> {
            let p = prefix(&[kind::SECRET, secret_name.as_str()]);
            self.journal.append(vec![Op::RemovePrefix(p)]).await?;
            Ok(self.memory.delete_secrets_from_inbox(secret_name).await?)
        }

        async fn set_custom_value(&self, name: &str, value: Vec<u8>) -> Result<(), StoreFail> {
            self.journal.append(vec![custom_op(name, &value)?]).await?;
            Ok(self.memory.set_custom_value(name, value).await?)
        }

        async fn remove_custom_value(&self, name: &str) -> Result<(), StoreFail> {
            self.journal.append(vec![Op::Remove(key(&[kind::CUSTOM, name]))]).await?;
            Ok(self.memory.remove_custom_value(name).await?)
        }

        async fn get_user_devices(
            &self,
            user_id: &UserId,
        ) -> Result<HashMap<OwnedDeviceId, DeviceData>, StoreFail> {
            Ok(self.memory.get_user_devices(user_id).await?)
        }
    }
    forward {
        fn load_account() -> Option<Account>;
        fn get_sessions(sender_key: &str) -> Option<Vec<Session>>;
        fn get_inbound_group_session(room_id: &RoomId, session_id: &str) -> Option<InboundGroupSession>;
        fn get_withheld_info(room_id: &RoomId, session_id: &str) -> Option<RoomKeyWithheldEntry>;
        fn get_withheld_sessions_by_room_id(room_id: &RoomId) -> Vec<RoomKeyWithheldEntry>;
        fn get_inbound_group_sessions() -> Vec<InboundGroupSession>;
        fn inbound_group_session_counts(backup_version: Option<&str>) -> RoomKeyCounts;
        fn get_inbound_group_sessions_by_room_id(room_id: &RoomId) -> Vec<InboundGroupSession>;
        fn get_inbound_group_sessions_for_device_batch(curve_key: Curve25519PublicKey, sender_data_type: SenderDataType, after_session_id: Option<String>, limit: usize) -> Vec<InboundGroupSession>;
        fn inbound_group_sessions_for_backup(backup_version: &str, limit: usize) -> Vec<InboundGroupSession>;
        fn load_backup_keys() -> BackupKeys;
        fn load_dehydrated_device_pickle_key() -> Option<DehydratedDeviceKey>;
        fn get_outbound_group_session(room_id: &RoomId) -> Option<OutboundGroupSession>;
        fn load_tracked_users() -> Vec<TrackedUser>;
        fn get_device(user_id: &UserId, device_id: &DeviceId) -> Option<DeviceData>;
        fn get_own_device() -> DeviceData;
        fn get_user_identity(user_id: &UserId) -> Option<UserIdentityData>;
        fn is_message_known(message_hash: &matrix_sdk_base::crypto::olm::OlmMessageHash) -> bool;
        fn get_outgoing_secret_requests(request_id: &TransactionId) -> Option<GossipRequest>;
        fn get_secret_request_by_info(secret_info: &SecretInfo) -> Option<GossipRequest>;
        fn get_unsent_secret_requests() -> Vec<GossipRequest>;
        fn get_secrets_from_inbox(secret_name: &SecretName) -> Vec<Zeroizing<String>>;
        fn get_room_settings(room_id: &RoomId) -> Option<RoomSettings>;
        fn get_received_room_key_bundle_data(room_id: &RoomId, user_id: &UserId) -> Option<StoredRoomKeyBundleData>;
        fn get_pending_key_bundle_details_for_room(room_id: &RoomId) -> Option<RoomPendingKeyBundleDetails>;
        fn get_all_rooms_pending_key_bundles() -> Vec<RoomPendingKeyBundleDetails>;
        fn has_downloaded_all_room_keys(room_id: &RoomId) -> bool;
        fn get_custom_value(key: &str) -> Option<Vec<u8>>;
        fn try_take_leased_lock(lease_duration_ms: u32, key: &str, holder: &str) -> Option<CrossProcessLockGeneration>;
        fn next_batch_token() -> Option<String>;
        fn close() -> ();
        fn reopen() -> ();
        fn get_size() -> Option<usize>;
    }
}
