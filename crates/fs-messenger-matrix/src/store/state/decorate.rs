//! The `StateStore` trait over the memory store and the journal.
//!
//! Pattern: Decorator: writes that must survive go to the journal first; the send queue and thread
//! subscriptions stay in memory, because this crate uses neither (`DESIGN.md`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use matrix_sdk_base::deserialized_responses::{DisplayName, RawAnySyncOrStrippedState};
use matrix_sdk_base::ruma::events::presence::PresenceEvent;
use matrix_sdk_base::ruma::events::receipt::{Receipt, ReceiptThread, ReceiptType};
use matrix_sdk_base::ruma::events::{
    AnyGlobalAccountDataEvent, AnyRoomAccountDataEvent, GlobalAccountDataEventType,
    RoomAccountDataEventType, StateEventType,
};
use matrix_sdk_base::ruma::profile::UserProfile;
use matrix_sdk_base::ruma::serde::Raw;
use matrix_sdk_base::ruma::{
    EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedTransactionId,
    OwnedUserId, RoomId, TransactionId, UserId,
};
use matrix_sdk_base::store::{
    ChildTransactionId, DependentQueuedRequest, DependentQueuedRequestKind, QueueWedgeError,
    QueuedRequest, QueuedRequestKind, RoomLoadSettings, SentRequestKey, StateStoreDataKey,
    StateStoreDataValue, StoredThreadSubscription,
};
use matrix_sdk_base::{
    MinimalRoomMemberEvent, RoomInfo, RoomMemberships, StateChanges, StateStore,
};

use super::FsStateStore;
use super::persist::{change_ops, custom_key, kv_key, room_ops};
use crate::error::StoreFail;
use crate::store::Op;
use crate::store::crypto::encode_bytes;
use crate::store::forward::decorate_memory;

fn hex_value(value: &[u8]) -> Result<String, StoreFail> {
    Ok(serde_json::to_string(&encode_bytes(value))?)
}

decorate_memory! {
    impl StateStore, for FsStateStore, error StoreFail;
    explicit {
        async fn set_kv_data(
            &self,
            key: StateStoreDataKey<'_>,
            value: StateStoreDataValue,
        ) -> Result<(), StoreFail> {
            let text = match &value {
                StateStoreDataValue::SyncToken(token) | StateStoreDataValue::Filter(token) => {
                    Some(serde_json::to_string(token)?)
                }
                StateStoreDataValue::OneTimeKeyAlreadyUploaded => Some("true".to_owned()),
                _ => None,
            };
            if let (Some(k), Some(text)) = (kv_key(&key), text) {
                self.journal.append(vec![Op::Put(k, text)]).await?;
            }
            Ok(self.memory.set_kv_data(key, value).await?)
        }

        async fn remove_kv_data(&self, key: StateStoreDataKey<'_>) -> Result<(), StoreFail> {
            if let Some(k) = kv_key(&key) {
                self.journal.append(vec![Op::Remove(k)]).await?;
            }
            Ok(self.memory.remove_kv_data(key).await?)
        }

        async fn save_changes(&self, changes: &StateChanges) -> Result<(), StoreFail> {
            self.journal.append(change_ops(changes)?).await?;
            Ok(self.memory.save_changes(changes).await?)
        }

        async fn get_profiles<'a>(
            &self,
            room_id: &RoomId,
            user_ids: &'a [OwnedUserId],
        ) -> Result<BTreeMap<&'a UserId, MinimalRoomMemberEvent>, StoreFail> {
            Ok(self.memory.get_profiles(room_id, user_ids).await?)
        }

        async fn get_users_with_display_names<'a>(
            &self,
            room_id: &RoomId,
            display_names: &'a [DisplayName],
        ) -> Result<HashMap<&'a DisplayName, BTreeSet<OwnedUserId>>, StoreFail> {
            Ok(self.memory.get_users_with_display_names(room_id, display_names).await?)
        }

        async fn get_global_profiles<'a>(
            &self,
            user_ids: &'a [OwnedUserId],
        ) -> Result<BTreeMap<&'a UserId, UserProfile>, StoreFail> {
            Ok(self.memory.get_global_profiles(user_ids).await?)
        }

        async fn set_custom_value(
            &self,
            key: &[u8],
            value: Vec<u8>,
        ) -> Result<Option<Vec<u8>>, StoreFail> {
            let op = Op::Put(custom_key(key), hex_value(&value)?);
            self.journal.append(vec![op]).await?;
            Ok(self.memory.set_custom_value(key, value).await?)
        }

        async fn set_custom_value_no_read(
            &self,
            key: &[u8],
            value: Vec<u8>,
        ) -> Result<(), StoreFail> {
            let op = Op::Put(custom_key(key), hex_value(&value)?);
            self.journal.append(vec![op]).await?;
            Ok(self.memory.set_custom_value_no_read(key, value).await?)
        }

        async fn remove_custom_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreFail> {
            self.journal.append(vec![Op::Remove(custom_key(key))]).await?;
            Ok(self.memory.remove_custom_value(key).await?)
        }

        async fn remove_room(&self, room_id: &RoomId) -> Result<(), StoreFail> {
            self.journal.append(room_ops(room_id.as_str())).await?;
            Ok(self.memory.remove_room(room_id).await?)
        }

        async fn upsert_thread_subscriptions(
            &self,
            updates: Vec<(&RoomId, &EventId, StoredThreadSubscription)>,
        ) -> Result<(), StoreFail> {
            Ok(self.memory.upsert_thread_subscriptions(updates).await?)
        }
    }
    forward {
        fn get_kv_data(key: StateStoreDataKey<'_>) -> Option<StateStoreDataValue>;
        fn get_presence_event(user_id: &UserId) -> Option<Raw<PresenceEvent>>;
        fn get_presence_events(user_ids: &[OwnedUserId]) -> Vec<Raw<PresenceEvent>>;
        fn get_state_event(room_id: &RoomId, event_type: StateEventType, state_key: &str) -> Option<RawAnySyncOrStrippedState>;
        fn get_state_events(room_id: &RoomId, event_type: StateEventType) -> Vec<RawAnySyncOrStrippedState>;
        fn get_state_events_for_keys(room_id: &RoomId, event_type: StateEventType, state_keys: &[&str]) -> Vec<RawAnySyncOrStrippedState>;
        fn get_profile(room_id: &RoomId, user_id: &UserId) -> Option<MinimalRoomMemberEvent>;
        fn get_user_ids(room_id: &RoomId, memberships: RoomMemberships) -> Vec<OwnedUserId>;
        fn get_room_infos(room_load_settings: &RoomLoadSettings) -> Vec<RoomInfo>;
        fn get_users_with_display_name(room_id: &RoomId, display_name: &DisplayName) -> BTreeSet<OwnedUserId>;
        fn get_account_data_event(event_type: GlobalAccountDataEventType) -> Option<Raw<AnyGlobalAccountDataEvent>>;
        fn get_room_account_data_event(room_id: &RoomId, event_type: RoomAccountDataEventType) -> Option<Raw<AnyRoomAccountDataEvent>>;
        fn get_user_room_receipt_event(room_id: &RoomId, receipt_type: ReceiptType, receipt_thread: &ReceiptThread, user_id: &UserId) -> Option<(OwnedEventId, Receipt)>;
        fn get_event_room_receipt_events(room_id: &RoomId, receipt_type: ReceiptType, receipt_thread: &ReceiptThread, event_id: &EventId) -> Vec<(OwnedUserId, Receipt)>;
        fn get_custom_value(key: &[u8]) -> Option<Vec<u8>>;
        fn save_send_queue_request(room_id: &RoomId, transaction_id: OwnedTransactionId, created_at: MilliSecondsSinceUnixEpoch, request: QueuedRequestKind, priority: usize) -> ();
        fn update_send_queue_request(room_id: &RoomId, transaction_id: &TransactionId, content: QueuedRequestKind) -> bool;
        fn remove_send_queue_request(room_id: &RoomId, transaction_id: &TransactionId) -> bool;
        fn load_send_queue_requests(room_id: &RoomId) -> Vec<QueuedRequest>;
        fn update_send_queue_request_status(room_id: &RoomId, transaction_id: &TransactionId, error: Option<QueueWedgeError>) -> ();
        fn load_rooms_with_unsent_requests() -> Vec<OwnedRoomId>;
        fn save_dependent_queued_request(room_id: &RoomId, parent_txn_id: &TransactionId, own_txn_id: ChildTransactionId, created_at: MilliSecondsSinceUnixEpoch, content: DependentQueuedRequestKind) -> ();
        fn mark_dependent_queued_requests_as_ready(room_id: &RoomId, parent_txn_id: &TransactionId, sent_parent_key: SentRequestKey) -> usize;
        fn update_dependent_queued_request(room_id: &RoomId, own_transaction_id: &ChildTransactionId, new_content: DependentQueuedRequestKind) -> bool;
        fn remove_dependent_queued_request(room: &RoomId, own_txn_id: &ChildTransactionId) -> bool;
        fn load_dependent_queued_requests(room: &RoomId) -> Vec<DependentQueuedRequest>;
        fn remove_thread_subscription(room: &RoomId, thread_id: &EventId) -> ();
        fn load_thread_subscription(room: &RoomId, thread_id: &EventId) -> Option<StoredThreadSubscription>;
        fn get_global_profile(user_id: &UserId) -> Option<UserProfile>;
        fn close() -> ();
        fn reopen() -> ();
        fn optimize() -> ();
        fn get_size() -> Option<usize>;
    }
}
