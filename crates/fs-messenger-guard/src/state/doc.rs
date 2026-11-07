//! [`GuardDoc`]: the one document that holds the guard's state.
//!
//! Pattern: State (data side). One document, so a change is atomic: a settled call, its used proof and
//! its release are written together or not at all. It reads strictly: whatever does not parse is
//! damage, and damage reads as stopped. Arguments are never kept, only their digest. Only public keys
//! are held.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use fs_store::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::stage::CallStage;
use crate::device::DeviceVerifier;
use crate::digest::unhex;
use crate::model::{AiGrant, GuardMode, GuardState};

const VERSION: u32 = 1;
/// Settled calls kept per participant; what is older is gone with its note. Provisional.
const KEEP_SETTLED: usize = 64;

/// A person's device key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceKey {
    pub(crate) key_type: String,
    /// Hex.
    pub(crate) public_key: String,
}

/// Who invited a participant, and so whose decision counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Invitation {
    pub(crate) by: String,
    pub(crate) at: DateTime<Utc>,
    /// No new question before this: a person refused one.
    #[serde(default)]
    pub(crate) paused_until: Option<DateTime<Utc>>,
    /// When the current hour of asking began, and how often it asked in it.
    #[serde(default)]
    pub(crate) asked_since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) asked: u32,
}

/// What the engine knows about one call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CallRecord {
    pub(crate) id: String,
    pub(crate) caller: String,
    pub(crate) capability: String,
    /// Hex.
    pub(crate) digest: String,
    /// Whether the digest covers input. The risk is derived again from this and the table.
    pub(crate) has_input: bool,
    pub(crate) revision: u64,
    /// The conversation of the caller's turn, which the digest covers.
    #[serde(default)]
    pub(crate) turn: Option<ObjectId>,
    pub(crate) stage: CallStage,
    pub(crate) asked_of: String,
    pub(crate) conversation: ObjectId,
    pub(crate) message: ObjectId,
    pub(crate) expires_at: DateTime<Utc>,
    /// The object read is marked: asks every time, can never be released. Shown as `unknown` until the
    /// contract has `readsSensitive` (mirrored locally).
    #[serde(default)]
    pub(crate) sensitive: bool,
}

/// The stored state. Field by field required: a missing `mode` is damage, not a default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuardDoc {
    pub(crate) version: u32,
    /// Hex. In every signed message; a new one kills every older proof.
    pub(crate) engine: String,
    /// The owner, from the first enrolment. Another one in the configuration is damage.
    pub(crate) owner: String,
    pub(crate) mode: GuardMode,
    pub(crate) stopped_by: String,
    pub(crate) stopped_at: Option<DateTime<Utc>>,
    pub(crate) devices: BTreeMap<String, Vec<DeviceKey>>,
    pub(crate) invited: BTreeMap<String, Invitation>,
    pub(crate) grants: Vec<AiGrant>,
    pub(crate) calls: Vec<CallRecord>,
    /// Keys of used proofs, for good.
    pub(crate) used_proofs: BTreeSet<String>,
}

impl GuardDoc {
    /// A new state: stopped, nobody invited, nothing released, a new identity.
    pub(crate) fn fresh(engine: String, owner: String) -> Self {
        Self {
            version: VERSION,
            engine,
            owner,
            mode: GuardMode::Stopped,
            stopped_by: String::new(),
            stopped_at: None,
            devices: BTreeMap::new(),
            invited: BTreeMap::new(),
            grants: vec![],
            calls: vec![],
            used_proofs: BTreeSet::new(),
        }
    }

    /// Read strictly. A record that says `open` and could not be stopped is no state: whatever would
    /// make a proof impossible is damage, and damage reads as stopped.
    pub(crate) fn from_value(
        value: Value,
        owner: &str,
        verifier: &dyn DeviceVerifier,
    ) -> Result<Self, String> {
        // Serde takes `{"open":null}` for the variant `open`.
        if !value.get("mode").is_some_and(Value::is_string) {
            return Err("the mode is not a plain string".into());
        }
        let doc: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if doc.version != VERSION {
            return Err(format!("version {} is not known here", doc.version));
        }
        if doc.engine.len() != 64 || unhex(&doc.engine).is_none() {
            return Err("the engine identity is not 64 hex characters".into());
        }
        if doc.owner != owner {
            return Err("the state belongs to another owner".into());
        }
        for keys in doc.devices.values() {
            for key in keys {
                let usable = key.key_type == verifier.key_type()
                    && unhex(&key.public_key).is_some_and(|k| verifier.accepts_key(&k));
                if !usable {
                    return Err("a device key cannot be used".into());
                }
            }
        }
        if doc.devices.get(owner).is_none_or(Vec::is_empty) {
            return Err("the owner has no key, so it could never be stopped or opened".into());
        }
        Ok(doc)
    }

    pub(crate) fn to_value(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|e| e.to_string())
    }

    pub(crate) fn is_invited(&self, profile: &str) -> bool {
        self.invited.contains_key(profile)
    }

    pub(crate) fn view(&self) -> GuardState {
        GuardState {
            mode: self.mode,
            grants: self.grants.clone(),
            invited: self.invited.keys().cloned().collect(),
            stopped_by: self.stopped_by.clone(),
            stopped_at: self.stopped_at,
        }
    }

    /// Drop the oldest settled calls over the limit per participant. Answers their messages: the
    /// notes go too, so that a window that keeps refilling does not fill the store. Their ids then
    /// refuse as unknown.
    pub(crate) fn prune_settled(&mut self) -> Vec<ObjectId> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for call in self.calls.iter().filter(|c| c.stage.is_settled()) {
            *counts.entry(call.caller.clone()).or_default() += 1;
        }
        let mut pruned = vec![];
        self.calls.retain(|c| {
            if c.stage.is_settled() {
                let held = counts.entry(c.caller.clone()).or_default();
                if *held > KEEP_SETTLED {
                    *held -= 1;
                    pruned.push(c.message);
                    return false;
                }
            }
            true
        });
        pruned
    }
}
