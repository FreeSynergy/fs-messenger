//! Rooms by hand: what is waiting as an invitation, joining, opening an encrypted room, room keys.
//!
//! Pattern: Facade over the SDK. The core has no room concept, so these are the provider's own. An
//! invitation is never accepted on its own: anyone can send one, and joining shows them our devices.

use std::path::PathBuf;

use matrix_sdk::Room;
use matrix_sdk::ruma::api::client::room::create_room;
use matrix_sdk::ruma::events::InitialStateEvent;
use matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent;
use matrix_sdk::ruma::{OwnedUserId, RoomId};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::secret::Password;

/// A room this account is invited to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    /// The room id.
    pub room_id: String,
    /// Who invited, if the server said.
    pub inviter: Option<String>,
}

impl MatrixProvider {
    /// What the last fetch saw as invitations. The core has no such concept: call this after a fetch.
    pub async fn invites(&self) -> Result<Vec<Invite>> {
        let mut invites = Vec::new();
        for room in self.client.invited_rooms() {
            let details = self.account.bounded(room.invite_details()).await?;
            invites.push(Invite {
                room_id: room.room_id().to_string(),
                inviter: details.inviter.map(|m| m.user_id().to_string()),
            });
        }
        Ok(invites)
    }

    /// Join a room this account is invited to. Keys sent before joining are not requested.
    pub async fn accept_invite(&self, room_id: &str) -> Result<()> {
        let room = self.room(room_id)?;
        self.account.bounded(room.join()).await
    }

    /// Open a new end-to-end encrypted room and invite these people. Answers its id.
    pub async fn create_encrypted_room(&self, invite: &[String]) -> Result<String> {
        let mut users = Vec::with_capacity(invite.len());
        for user in invite {
            let id = user
                .as_str()
                .try_into()
                .map_err(|_: matrix_sdk::IdParseError| {
                    MatrixError::Config(format!("not a Matrix id: {user}"))
                })?;
            users.push(id);
        }
        let room = self.open_room(users).await?;
        Ok(room.room_id().to_string())
    }

    pub(super) async fn open_room(&self, invite: Vec<OwnedUserId>) -> Result<Room> {
        let encryption = InitialStateEvent::with_empty_state_key(
            RoomEncryptionEventContent::with_recommended_defaults(),
        );
        let mut request = create_room::v3::Request::new();
        request.invite = invite;
        request.preset = Some(create_room::v3::RoomPreset::PrivateChat);
        request.initial_state = vec![encryption.to_raw_any()];
        self.account.bounded(self.client.create_room(request)).await
    }

    /// Write this device's room keys to a file, encrypted with `passphrase`. There is no key backup
    /// on the server (`DESIGN.md`); this is the way to carry keys to another device.
    pub async fn export_room_keys(&self, path: PathBuf, passphrase: &Password) -> Result<()> {
        let encryption = self.client.encryption();
        Ok(encryption
            .export_room_keys(path, passphrase.expose(), |_| true)
            .await?)
    }

    /// Read room keys from a file made by `export_room_keys`. Answers how many were new. Messages
    /// waiting for one of them open on the next fetch.
    pub async fn import_room_keys(&self, path: PathBuf, passphrase: &Password) -> Result<usize> {
        let encryption = self.client.encryption();
        let result = encryption.import_room_keys(path, passphrase.expose()).await;
        let imported = result.map_err(|e| MatrixError::Sdk(e.to_string()))?;
        Ok(imported.imported_count)
    }

    fn room(&self, room_id: &str) -> Result<Room> {
        let id = <&RoomId>::try_from(room_id)
            .map_err(|_| MatrixError::Config(format!("not a room: {room_id}")))?;
        self.client
            .get_room(id)
            .ok_or_else(|| MatrixError::Sdk(format!("no such room here: {room_id}")))
    }
}
