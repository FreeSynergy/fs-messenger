//! What a person changes: invite, release, revoke, stop, reopen, devices.
//!
//! Pattern: State: pure transitions on a copy; the engine writes the result or nothing. Release takes
//! only what `risk` says can be released. Stop and reopen both clear the releases, so a record that
//! held on to one through a stop does not come back.

use chrono::{DateTime, Utc};
use fs_messenger_core::ToolCallRisk;
use serde::{Deserialize, Serialize};

use super::doc::{DeviceKey, GuardDoc, Invitation};
use super::stage::CallStage;
use crate::arguments::hides_text;
use crate::error::Refusal;
use crate::model::{AiGrant, GuardMode};
use crate::risk::can_be_released;

const MAX_NAME_BYTES: usize = 2048;

/// A name that goes into what a person reads and signs: nothing in it can hide or reorder text.
pub(crate) fn usable_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.contains(hides_text)
}

/// Who pulled the stop, when, and the proof it used up: whatever is written later, that proof has
/// been used.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StopMark {
    pub(crate) by: String,
    pub(crate) at: DateTime<Utc>,
    pub(crate) used: Option<String>,
}

/// A device key to add.
pub(crate) struct DeviceChange<'a> {
    pub(crate) person: &'a str,
    pub(crate) key_type: &'a str,
    pub(crate) public_key: String,
}

impl GuardDoc {
    fn require_open(&self) -> Result<(), Refusal> {
        if self.mode == GuardMode::Open {
            Ok(())
        } else {
            Err(Refusal::Stopped)
        }
    }

    /// May this person act for this participant? The person who invited it, or the owner.
    fn require_inviter(&self, profile: &str, signer: &str, owner: &str) -> Result<(), Refusal> {
        let invitation = self.invited.get(profile).ok_or(Refusal::NotInvited)?;
        if signer == invitation.by || signer == owner {
            Ok(())
        } else {
            Err(Refusal::NotPermitted)
        }
    }

    pub(crate) fn check_invite(&self, profile: &str, owner: &str) -> Result<(), Refusal> {
        self.require_open()?;
        if !usable_name(profile) {
            return Err(Refusal::BadName);
        }
        if self.is_invited(profile) {
            return Err(Refusal::AlreadyInvited);
        }
        if profile == owner || self.devices.contains_key(profile) {
            return Err(Refusal::IdentityConflict);
        }
        Ok(())
    }

    pub(crate) fn apply_invite(&mut self, profile: &str, by: &str, at: DateTime<Utc>) {
        self.invited.insert(
            profile.to_string(),
            Invitation {
                by: by.to_string(),
                at,
                paused_until: None,
                asked_since: None,
                asked: 0,
            },
        );
    }

    pub(crate) fn check_release(
        &self,
        grant: &AiGrant,
        signer: &str,
        owner: &str,
    ) -> Result<(), Refusal> {
        self.require_open()?;
        if !can_be_released(grant.risk) {
            return Err(Refusal::NotReleasable(grant.risk));
        }
        self.require_inviter(&grant.profile, signer, owner)
    }

    pub(crate) fn apply_grant(&mut self, grant: AiGrant) {
        if !self.grants.contains(&grant) {
            self.grants.push(grant);
        }
    }

    /// Is this class released for this participant? Only what can be released at all counts, whatever
    /// a record says.
    pub(crate) fn is_released(&self, profile: &str, risk: ToolCallRisk) -> bool {
        can_be_released(risk)
            && self
                .grants
                .iter()
                .any(|g| g.profile == profile && g.risk == risk)
    }

    pub(crate) fn check_revoke(
        &self,
        profile: &str,
        signer: &str,
        owner: &str,
    ) -> Result<(), Refusal> {
        self.require_inviter(profile, signer, owner)
    }

    /// Takes the releases and the open calls of this participant. Answers the calls it settled.
    pub(crate) fn apply_revoke(&mut self, profile: &str) -> Vec<String> {
        self.grants.retain(|g| g.profile != profile);
        let mut cancelled = vec![];
        for call in self.calls.iter_mut().filter(|c| c.caller == profile) {
            if !call.stage.is_settled() {
                call.stage = CallStage::Cancelled;
                call.revision += 1;
                cancelled.push(call.id.clone());
            }
        }
        cancelled
    }

    /// Withdraws an invitation, with what hangs on it. Answers the calls it settled.
    pub(crate) fn apply_uninvite(&mut self, profile: &str) -> Vec<String> {
        let cancelled = self.apply_revoke(profile);
        self.invited.remove(profile);
        cancelled
    }

    /// Stops: mode, releases, every open call. Answers how many calls it settled.
    pub(crate) fn apply_stop(&mut self, mark: &StopMark) -> usize {
        if let Some(used) = &mark.used {
            self.used_proofs.insert(used.clone());
        }
        if self.mode == GuardMode::Open {
            self.stopped_by.clone_from(&mark.by);
            self.stopped_at = Some(mark.at);
        }
        self.mode = GuardMode::Stopped;
        self.grants.clear();
        let mut cancelled = 0;
        for call in &mut self.calls {
            if !call.stage.is_settled() {
                call.stage = CallStage::Cancelled;
                call.revision += 1;
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Opens. Releases stay taken: also one that is stored against the rules.
    pub(crate) fn apply_reopen(&mut self) {
        self.mode = GuardMode::Open;
        self.stopped_by.clear();
        self.stopped_at = None;
        self.grants.clear();
    }

    /// May this person get a key? It comes with the owner's signature, so a person's identity wins over
    /// an invitation somebody else made of the same profile. An invitation by the owner stays: the
    /// owner said it is an AI, and has to withdraw that first.
    pub(crate) fn check_device(&self, person: &str, owner: &str) -> Result<(), Refusal> {
        if !usable_name(person) {
            return Err(Refusal::BadName);
        }
        match self.invited.get(person) {
            Some(invitation) if invitation.by == owner => Err(Refusal::IdentityConflict),
            _ if owner.is_empty() => Err(Refusal::IdentityConflict),
            _ => Ok(()),
        }
    }

    pub(crate) fn add_device(&mut self, change: DeviceChange<'_>) {
        let key = DeviceKey {
            key_type: change.key_type.to_string(),
            public_key: change.public_key,
        };
        let keys = self.devices.entry(change.person.to_string()).or_default();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    /// The owner's last key cannot go: that would leave a program nobody can stop by proof or open.
    pub(crate) fn remove_device(
        &mut self,
        person: &str,
        public_key: &str,
        owner: &str,
    ) -> Result<(), Refusal> {
        let keys = self.devices.get_mut(person).ok_or(Refusal::BadKey)?;
        if !keys.iter().any(|k| k.public_key == public_key) {
            return Err(Refusal::BadKey);
        }
        if person == owner && keys.len() == 1 {
            return Err(Refusal::LastOwnerKey);
        }
        keys.retain(|k| k.public_key != public_key);
        if keys.is_empty() {
            self.devices.remove(person);
        }
        Ok(())
    }
}
