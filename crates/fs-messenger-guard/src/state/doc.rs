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
use crate::model::{AiGrant, GuardMode, GuardState};

const VERSION: u32 = 1;
/// Settled calls kept to show; what is older is gone, and its id refuses as unknown. Provisional.
const KEEP_SETTLED: usize = 256;

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
    pub(crate) stage: CallStage,
    pub(crate) asked_of: String,
    pub(crate) conversation: ObjectId,
    pub(crate) message: ObjectId,
    pub(crate) expires_at: DateTime<Utc>,
}

/// The stored state. Field by field required: a missing `mode` is damage, not a default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuardDoc {
    pub(crate) version: u32,
    /// Hex. In every signed message; a new one kills every older proof.
    pub(crate) engine: String,
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
    pub(crate) fn fresh(engine: String) -> Self {
        Self {
            version: VERSION,
            engine,
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

    pub(crate) fn from_value(value: Value) -> Result<Self, String> {
        let doc: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if doc.version != VERSION {
            return Err(format!("version {} is not known here", doc.version));
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

    /// Drop the oldest settled calls over the limit. Their ids then refuse as unknown.
    pub(crate) fn prune_settled(&mut self) {
        let settled = self.calls.iter().filter(|c| c.stage.is_settled()).count();
        let mut over = settled.saturating_sub(KEEP_SETTLED);
        self.calls.retain(|c| {
            if over > 0 && c.stage.is_settled() {
                over -= 1;
                return false;
            }
            true
        });
    }
}
