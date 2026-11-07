//! Sending: find the room, check it again, send, report what really happened.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). The
//! core's plan went first, but the plan only sees recipients and not the room a reply goes to, so
//! the provider checks the room itself. Nothing reaches `Room::send` unless the room is encrypted
//! right now, and the SDK only leaves encrypted rooms with encrypted events.

use std::future::IntoFuture;

use fs_interfaces::provider::Unsupported;
use fs_messenger_core::{
    Encryption, Error as CoreError, ExternalId, Outgoing, Result as CoreResult, SendReceipt,
};
use matrix_sdk::Room;
use matrix_sdk::deserialized_responses::AlgorithmInfo;
use matrix_sdk::ruma::events::relation::Reply;
use matrix_sdk::ruma::events::room::message::{Relation, RoomMessageEventContent};
use matrix_sdk::ruma::{OwnedEventId, RoomId, UserId};

use super::MatrixProvider;
use crate::error::MatrixError;
use crate::receive::{EventAlias, SCHEME_ROOM};

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::Invalid(why.into())
}

fn sdk(e: impl Into<MatrixError>) -> CoreError {
    e.into().into()
}

/// The parent to answer: an alias this provider issued, for an event of this very room.
fn parent_of(message: &Outgoing, room: &Room) -> Option<OwnedEventId> {
    let ids = message.in_reply_to().iter();
    ids.filter_map(|a: &ExternalId| EventAlias::from_external(a))
        .filter(|alias| alias.room() == room.room_id().as_str())
        .find_map(|alias| alias.event().try_into().ok())
}

impl MatrixProvider {
    pub(super) async fn send_message(&self, message: &Outgoing) -> CoreResult<SendReceipt> {
        if !message.bcc().is_empty() {
            let why = "a Matrix room has no hidden members; nothing left";
            return Err(Unsupported::new("message.bcc", why).into());
        }
        if !message.attachments().is_empty() {
            let why = "encrypted attachments are not built; nothing left";
            return Err(Unsupported::new("attachment.send", why).into());
        }
        let room = self.target_room(message).await?;
        let verdict = self.room_verdict(&room).await.map_err(sdk)?;
        if !verdict.passes() {
            return Err(CoreError::EncryptionRefused {
                encryption: verdict.encryption,
                missing_keys: verdict.missing,
            });
        }

        let mut content = RoomMessageEventContent::text_plain(message.text());
        if let Some(parent) = parent_of(message, &room) {
            content.relates_to = Some(Relation::Reply(Reply::with_event_id(parent)));
        }
        // Held across the send: see `echo.rs`.
        let mut sent = self.sent.lock().await;
        let delivery = room.send(content).into_future();
        let result = self.account.bounded(delivery).await.map_err(sdk)?;
        let alias = EventAlias::new(room.room_id().as_str(), result.response.event_id.as_str());
        sent.remember(alias.clone());
        drop(sent);

        let encryption = match result.encryption_info.as_ref().map(|i| &i.algorithm_info) {
            Some(AlgorithmInfo::MegolmV1AesSha2 { .. }) => Encryption::Megolm,
            Some(_) => Encryption::Unknown,
            None => Encryption::None,
        };
        Ok(SendReceipt {
            aliases: vec![alias.external()],
            conversation: Some(ExternalId::new(SCHEME_ROOM, room.room_id().as_str())),
            encryption,
        })
    }

    /// The conversation's room; or the one recipient room; or the direct room with the one person,
    /// opened if there is none; or a new room for several people. Opened rooms are always encrypted.
    async fn target_room(&self, message: &Outgoing) -> CoreResult<Room> {
        let known = message
            .conversation()
            .iter()
            .find(|a| a.scheme == SCHEME_ROOM);
        if let Some(alias) = known {
            return self.known_room(&alias.id);
        }
        let everyone = [message.to(), message.cc()].concat();
        if let [only] = everyone.as_slice()
            && only.starts_with('!')
        {
            return self.known_room(only);
        }
        let mut users = Vec::with_capacity(everyone.len());
        for recipient in &everyone {
            let user = <&UserId>::try_from(recipient.as_str())
                .map_err(|_| invalid(format!("not one room or Matrix ids: {recipient}")))?;
            users.push(user.to_owned());
        }
        if let [only] = users.as_slice() {
            if let Some(room) = self.direct_room(only).await.map_err(sdk)? {
                return Ok(room);
            }
            return self
                .account
                .bounded(self.client.create_dm(only))
                .await
                .map_err(sdk);
        }
        self.open_room(users).await.map_err(sdk)
    }

    fn known_room(&self, id: &str) -> CoreResult<Room> {
        let room_id = <&RoomId>::try_from(id).map_err(|_| invalid(format!("not a room: {id}")))?;
        self.client
            .get_room(room_id)
            .ok_or_else(|| CoreError::EncryptionRefused {
                encryption: Encryption::Unknown,
                missing_keys: vec![id.to_owned()],
            })
    }
}
