//! [`Usage`]: what an agent has used in a conversation.
//!
//! Pattern: Value object. Zero means not known; a provider does not guess.

use serde::{Deserialize, Serialize};

/// What an agent has used. Zero or empty means not known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// Tokens read.
    pub input_tokens: u64,
    /// Tokens written.
    pub output_tokens: u64,
    /// Context in use.
    pub context_used: u64,
    /// Size of the context.
    pub context_size: u64,
    /// Cost in millionths of `currency`.
    pub cost_micros: u64,
    /// ISO 4217.
    pub currency: String,
}
