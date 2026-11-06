//! [`EncryptionPlan`]: the answer to "how would this go out?".
//!
//! Pattern: Value object, the contract's `encryptionPlan` result. The default is the fail-safe one:
//! no way found.

use serde::{Deserialize, Serialize};

use super::Encryption;

/// How a message would be protected, and who stands in the way.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EncryptionPlan {
    /// The way found; `unknown` until a provider says otherwise, and that is refused like `none`.
    pub encryption: Encryption,
    /// Recipients without a usable key.
    pub missing_keys: Vec<String>,
}
