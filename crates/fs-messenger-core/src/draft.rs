//! The [`Draft`] contract: a message being written.
//!
//! Pattern: State: a draft is open until `send` succeeds or `discard` runs; afterwards every call
//! answers an error. `send` has no override parameter: the rule of ADR 0018 is not negotiable here.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs_store::ObjectId;

use crate::error::Result;
use crate::model::EncryptionPlan;

/// A saved draft in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftInfo {
    /// Id of the draft.
    pub id: ObjectId,
    /// Primary recipients.
    pub to: Vec<String>,
    /// The text so far.
    pub text: String,
}

/// A message being written.
#[async_trait]
pub trait Draft: Send + Sync {
    /// Who receives it. Answers the recipients for whom no encrypted way is known.
    async fn set_recipients(
        &mut self,
        to: Vec<String>,
        cc: Vec<String>,
        bcc: Vec<String>,
    ) -> Result<Vec<String>>;
    /// The account it goes out from. Not in the contract yet; provisional, see `DESIGN.md`.
    async fn set_account(&mut self, account: ObjectId) -> Result<()>;
    /// The text: `text/plain` or `text/markdown`.
    async fn set_text(&mut self, text: String, media_type: String) -> Result<()>;
    /// Add a file; it is stored at once and the draft with it.
    async fn attach(&mut self, name: String, media_type: String, data: Vec<u8>) -> Result<()>;
    /// How it would go out, and who stands in the way (ADR 0018: find a way).
    async fn encryption_plan(&self) -> Result<EncryptionPlan>;
    /// Keep it; answers its id.
    async fn save(&mut self) -> Result<ObjectId>;
    /// Run the plan, then send. `Error::EncryptionRefused` means nothing left and the draft is
    /// saved. A time in the future is `Unsupported` for now: nothing would run it.
    async fn send(&mut self, at: Option<DateTime<Utc>>) -> Result<ObjectId>;
    /// Throw it away.
    async fn discard(&mut self) -> Result<()>;
}
