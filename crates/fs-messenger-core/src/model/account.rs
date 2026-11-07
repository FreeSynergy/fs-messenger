//! [`Account`] and [`AccountSetup`].
//!
//! Pattern: Value objects. The secret never appears in `Debug` output, and the core does not store
//! it: there is no key vault yet (ADR 0024), see `Messenger::add_account`.

use fs_objects::ObjectId;
use serde::{Deserialize, Serialize};

/// One account of the person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Id of the account.
    pub id: ObjectId,
    /// Address, for example an e-mail address or a Matrix id.
    pub address: String,
    /// `mail`, `matrix`, `mimi`, ...
    pub kind: String,
    /// Whether a registered provider says it can plan encryption for this account.
    pub encryption_ready: bool,
}

/// What it takes to add an account.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccountSetup {
    /// Address.
    pub address: String,
    /// `mail`, `matrix`, `mimi`, ...
    pub kind: String,
    /// Login secret. Refused while there is no key vault.
    pub secret: String,
}

impl std::fmt::Debug for AccountSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSetup")
            .field("address", &self.address)
            .field("kind", &self.kind)
            .field("secret", &"<redacted>")
            .finish()
    }
}
