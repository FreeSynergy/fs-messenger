//! What the stores keep across a restart, offline: the parts the SDK's suites do not read back after
//! reopening, and the journal itself.

mod support;

use std::sync::Arc;

use fs_messenger_matrix::StoreFail;
use fs_messenger_matrix::store::{FsCryptoStore, FsJournal, FsStateStore};
use fs_store::traits::ActivityLog;
use fs_store::{ActivityFilter, TursoStore};
use matrix_sdk_base::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk_base::store::StateStoreDataKey;
use matrix_sdk_base::{RoomInfo, RoomState, StateChanges, StateStore};
use matrix_sdk_crypto::store::CryptoStore;
use ruma::events::{GlobalAccountDataEventType, StateEventType};
use ruma::serde::Raw;
use ruma::{owned_event_id, room_id, user_id};
use serde_json::json;

async fn state_over(store: &Arc<TursoStore>) -> FsStateStore {
    let journal = FsJournal::open(store.clone(), "state").await.unwrap();
    FsStateStore::open(Arc::new(journal)).await.unwrap()
}

async fn crypto_over(store: &Arc<TursoStore>) -> FsCryptoStore {
    let journal = FsJournal::open(store.clone(), "crypto").await.unwrap();
    FsCryptoStore::open(Arc::new(journal)).await.unwrap()
}

fn topic(event_id: &str, text: &str) -> serde_json::Value {
    json!({
        "type": "m.room.topic", "state_key": "", "event_id": event_id, "sender": "@a:x.test",
        "origin_server_ts": 1, "content": {"topic": text},
    })
}

#[tokio::test]
async fn a_plain_store_is_refused() {
    let plain = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let refused = FsJournal::open(plain, "state").await.unwrap_err();
    assert!(matches!(refused, StoreFail::NotEncrypted), "{refused}");
}

#[tokio::test]
async fn state_comes_back_and_goes_with_its_room() {
    let store = support::fresh_store().await;
    let room = room_id!("!r:x.test");
    let other = room_id!("!r:x.test.other");

    let state = state_over(&store).await;
    let mut changes = StateChanges::new("token-1".to_owned());
    changes.add_room(RoomInfo::new(room, RoomState::Joined));
    changes.add_room(RoomInfo::new(other, RoomState::Joined));
    let raw_topic = Raw::new(&topic("$t:x", "Secret plans"))
        .unwrap()
        .cast_unchecked();
    for r in [room, other] {
        let types = changes.state.entry(r.to_owned()).or_default();
        let events = types.entry(StateEventType::RoomTopic).or_default();
        events.insert(String::new(), raw_topic.clone());
    }
    let data = Raw::new(&json!({"type": "m.direct", "content": {}}))
        .unwrap()
        .cast_unchecked();
    changes
        .account_data
        .insert(GlobalAccountDataEventType::Direct, data);
    state.save_changes(&changes).await.unwrap();
    state
        .set_custom_value_no_read(b"custom", vec![0, 255])
        .await
        .unwrap();
    drop(state);

    let state = state_over(&store).await;
    let token = state
        .get_kv_data(StateStoreDataKey::SyncToken)
        .await
        .unwrap();
    assert_eq!(token.unwrap().into_sync_token().as_deref(), Some("token-1"));
    let rooms = state.get_room_infos(&Default::default()).await.unwrap();
    assert_eq!(rooms.len(), 2);
    let found = state
        .get_state_event(room, StateEventType::RoomTopic, "")
        .await
        .unwrap();
    assert!(found.is_some());
    assert!(
        state
            .get_account_data_event(GlobalAccountDataEventType::Direct)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        state.get_custom_value(b"custom").await.unwrap(),
        Some(vec![0, 255])
    );

    // Removing a room stays removed, and leaves the room whose id only starts the same.
    state.remove_room(room).await.unwrap();
    state.remove_custom_value(b"custom").await.unwrap();
    drop(state);
    let state = state_over(&store).await;
    assert!(
        state
            .get_state_event(room, StateEventType::RoomTopic, "")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .get_state_event(other, StateEventType::RoomTopic, "")
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        state
            .get_room_infos(&Default::default())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(state.get_custom_value(b"custom").await.unwrap(), None);

    // Unlogged: the activity log holds nothing of all this.
    let logged = store.activities(&ActivityFilter::default()).await.unwrap();
    assert!(logged.is_empty(), "{} activities", logged.len());
}

#[tokio::test]
async fn a_redaction_still_holds_after_a_restart() {
    let store = support::fresh_store().await;
    let room = room_id!("!r:x.test");
    let state = state_over(&store).await;
    let mut changes = StateChanges::new("t".to_owned());
    changes.add_room(RoomInfo::new(room, RoomState::Joined));
    let raw_topic = Raw::new(&topic("$t:x", "Secret plans"))
        .unwrap()
        .cast_unchecked();
    let types = changes.state.entry(room.to_owned()).or_default();
    let events = types.entry(StateEventType::RoomTopic).or_default();
    events.insert(String::new(), raw_topic);
    state.save_changes(&changes).await.unwrap();

    let redaction = json!({
        "type": "m.room.redaction", "event_id": "$r:x", "sender": "@a:x.test", "redacts": "$t:x",
        "origin_server_ts": 2, "content": {"redacts": "$t:x"},
    });
    let mut changes = StateChanges::new("t2".to_owned());
    let redactions = changes.redactions.entry(room.to_owned()).or_default();
    redactions.insert(
        owned_event_id!("$t:x"),
        Raw::new(&redaction).unwrap().cast_unchecked(),
    );
    state.save_changes(&changes).await.unwrap();
    drop(state);

    let state = state_over(&store).await;
    let held = state
        .get_state_event(room, StateEventType::RoomTopic, "")
        .await
        .unwrap();
    let RawAnySyncOrStrippedState::Sync(raw) = held.expect("the event stays, emptied") else {
        panic!("a joined room has synced state");
    };
    assert!(
        !raw.json().get().contains("Secret plans"),
        "{}",
        raw.json().get()
    );
}

#[tokio::test]
async fn crypto_records_come_back_and_stay_gone() {
    let store = support::fresh_store().await;
    let alice = user_id!("@alice:x.test");
    let crypto = crypto_over(&store).await;
    crypto.save_tracked_users(&[(alice, true)]).await.unwrap();
    crypto.set_custom_value("a", vec![1, 2, 3]).await.unwrap();
    crypto.set_custom_value("b", vec![4]).await.unwrap();
    crypto.remove_custom_value("b").await.unwrap();
    drop(crypto);

    let crypto = crypto_over(&store).await;
    let tracked = crypto.load_tracked_users().await.unwrap();
    assert_eq!(tracked.len(), 1);
    assert!(tracked[0].dirty);
    assert_eq!(
        crypto.get_custom_value("a").await.unwrap(),
        Some(vec![1, 2, 3])
    );
    assert_eq!(crypto.get_custom_value("b").await.unwrap(), None);
}
