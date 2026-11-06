//! The [`Provider`] trait: mail, Matrix, MIMI, an agent ...
//!
//! Pattern: Strategy (ADR 0045 provider rule): like a parent class, every provider has every method.
//! What it does not override answers an explicit `Unsupported`. The core treats a provider as
//! untrusted: a provider without a plan cannot send, and it can only be handed an [`Outgoing`],
//! which only the core can build, after the check of ADR 0018.

use async_trait::async_trait;
use fs_interfaces::provider::{Capabilities, Unsupported};

use super::{FetchBatch, Outgoing, ProviderCursor, SendReceipt, TurnScope};
use crate::error::Result;
use crate::model::EncryptionPlan;

const NOT_OFFERED: &str = "this provider does not offer it";

/// A connector to one transport.
#[async_trait]
pub trait Provider: Send + Sync {
    /// `mail`, `matrix`, `mimi`, ...
    fn kind(&self) -> Result<String> {
        Err(Unsupported::new("provider.kind", NOT_OFFERED).into())
    }

    /// What it can do. Ask this rather than trying.
    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new("unnamed");
        for id in [
            "provider.kind",
            "message.fetch",
            "message.send",
            "encryption.plan",
            "turn.stop",
        ] {
            caps.unsupported(id, NOT_OFFERED);
        }
        caps
    }

    /// Messages since `since`; `None` is from the start. May repeat messages: the core dedupes.
    async fn fetch(&self, _since: Option<&ProviderCursor>) -> Result<FetchBatch> {
        Err(Unsupported::new("message.fetch", NOT_OFFERED).into())
    }

    /// Send what the core has approved. Report the encryption really used.
    async fn send(&self, _message: &Outgoing) -> Result<SendReceipt> {
        Err(Unsupported::new("message.send", NOT_OFFERED).into())
    }

    /// Look for an encrypted way to all of `recipients`, and name who stands in the way.
    async fn encryption_plan(&self, _recipients: &[String]) -> Result<EncryptionPlan> {
        Err(Unsupported::new("encryption.plan", NOT_OFFERED).into())
    }

    /// End running agent turns. Called by the guard only; it gets nothing back from here but the
    /// answer, and whoever cannot end a turn says so instead of pretending. Must not wait on a model:
    /// the guard gives up on it after a short time and reports that.
    async fn stop_turns(&self, _scope: &TurnScope) -> Result<()> {
        Err(Unsupported::new("turn.stop", NOT_OFFERED).into())
    }
}
