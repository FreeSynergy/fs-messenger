//! `encryption_plan`: is there really an encrypted way, and who stands in the way.
//!
//! Pattern: Specification over what the SDK knows now. The same check runs in `send`, so it holds even
//! if nobody asked for a plan. A recipient is a room (`!...`) or a person (`@...`).

use fs_messenger_core::{Encryption, EncryptionPlan};
use matrix_sdk::encryption::identities::Device;
use matrix_sdk::ruma::{EventEncryptionAlgorithm, RoomId, UserId};
use matrix_sdk::{Room, RoomMemberships, RoomState};

use super::MatrixProvider;
use crate::error::Result;

/// The way a room offers.
#[derive(Debug)]
pub(super) struct Verdict {
    pub(super) encryption: Encryption,
    pub(super) missing: Vec<String>,
}

impl Verdict {
    fn none(who: impl Into<String>, encryption: Encryption) -> Self {
        Self {
            encryption,
            missing: vec![who.into()],
        }
    }

    pub(super) fn passes(&self) -> bool {
        self.encryption == Encryption::Megolm && self.missing.is_empty()
    }
}

/// The weakest of two ways: a mix of rooms goes as well as its worst one.
fn weakest(a: Encryption, b: Encryption) -> Encryption {
    let rank = |e| match e {
        Encryption::Megolm => 2,
        Encryption::None => 1,
        _ => 0,
    };
    if rank(a) <= rank(b) { a } else { b }
}

impl MatrixProvider {
    pub(super) async fn plan_for(&self, recipients: &[String]) -> Result<EncryptionPlan> {
        let mut plan = EncryptionPlan {
            encryption: Encryption::Megolm,
            missing_keys: vec![],
        };
        for recipient in recipients {
            let verdict = self.recipient_verdict(recipient).await?;
            plan.encryption = weakest(plan.encryption, verdict.encryption);
            plan.missing_keys.extend(verdict.missing);
        }
        Ok(plan)
    }

    async fn recipient_verdict(&self, recipient: &str) -> Result<Verdict> {
        if let Ok(room_id) = <&RoomId>::try_from(recipient) {
            return match self.client.get_room(room_id) {
                Some(room) => self.room_verdict(&room).await,
                None => Ok(Verdict::none(recipient, Encryption::Unknown)),
            };
        }
        let Ok(user) = <&UserId>::try_from(recipient) else {
            return Ok(Verdict::none(recipient, Encryption::Unknown));
        };
        if let Some(room) = self.direct_room(user).await? {
            return self.room_verdict(&room).await;
        }
        // No room yet. `send` opens an encrypted one, if this person has keys to encrypt to.
        Ok(if self.has_usable_device(user).await? {
            Verdict {
                encryption: Encryption::Megolm,
                missing: vec![],
            }
        } else {
            Verdict::none(recipient, Encryption::Megolm)
        })
    }

    /// Can the SDK really encrypt into this room, for everyone in it?
    pub(super) async fn room_verdict(&self, room: &Room) -> Result<Verdict> {
        let id = room.room_id().as_str();
        if room.state() != RoomState::Joined {
            return Ok(Verdict::none(id, Encryption::Unknown));
        }
        if !self
            .account
            .bounded(room.latest_encryption_state())
            .await?
            .is_encrypted()
        {
            return Ok(Verdict::none(id, Encryption::None));
        }
        let algorithm = room.encryption_settings().and_then(|s| s.algorithm);
        if algorithm != Some(EventEncryptionAlgorithm::MegolmV1AesSha2) {
            return Ok(Verdict::none(id, Encryption::Unknown));
        }
        if self.client.encryption().ed25519_key().await.is_none() {
            let own = self.account.user_id.clone();
            return Ok(Verdict::none(own, Encryption::Unknown));
        }
        let mut missing = Vec::new();
        for member in self
            .account
            .bounded(room.members(RoomMemberships::ACTIVE))
            .await?
        {
            let user = member.user_id();
            if user.as_str() != self.account.user_id && !self.has_usable_device(user).await? {
                missing.push(user.to_string());
            }
        }
        Ok(Verdict {
            encryption: Encryption::Megolm,
            missing,
        })
    }

    /// Does this person have a device we would encrypt to? Asks the server if nothing is known.
    async fn has_usable_device(&self, user: &UserId) -> Result<bool> {
        let usable = |d: Device| !d.is_blacklisted();
        let encryption = self.client.encryption();
        if encryption
            .get_user_devices(user)
            .await?
            .devices()
            .any(usable)
        {
            return Ok(true);
        }
        self.account
            .bounded(encryption.request_user_identity(user))
            .await?;
        Ok(encryption
            .get_user_devices(user)
            .await?
            .devices()
            .any(usable))
    }

    /// A joined, encrypted direct room with this person, if there is one.
    pub(super) async fn direct_room(&self, user: &UserId) -> Result<Option<Room>> {
        let rooms: Vec<Room> = self.client.get_dm_rooms(user).collect();
        for room in rooms {
            if room.state() == RoomState::Joined
                && self
                    .account
                    .bounded(room.latest_encryption_state())
                    .await?
                    .is_encrypted()
            {
                return Ok(Some(room));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mix_goes_as_well_as_its_worst() {
        use Encryption::{Megolm, None, Unknown};
        assert_eq!(weakest(Megolm, Megolm), Megolm);
        assert_eq!(weakest(Megolm, None), None);
        assert_eq!(weakest(None, Unknown), Unknown);
    }

    #[test]
    fn only_megolm_without_gaps_passes() {
        let v = |encryption, missing: &[&str]| Verdict {
            encryption,
            missing: missing.iter().map(ToString::to_string).collect(),
        };
        assert!(v(Encryption::Megolm, &[]).passes());
        assert!(!v(Encryption::Megolm, &["@a:x"]).passes());
        assert!(!v(Encryption::None, &[]).passes());
        assert!(!v(Encryption::Unknown, &[]).passes());
    }
}
