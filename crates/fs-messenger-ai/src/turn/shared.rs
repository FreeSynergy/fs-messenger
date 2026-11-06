//! [`Shared`]: what the provider and its running turns hold together.
//!
//! Pattern: Facade over the parts of one provider. A turn runs in a task of its own, so these live behind
//! an `Arc`. The way back to the messenger is weak: the messenger holds the provider, and a cycle would
//! keep its store's claim for ever.

use std::sync::{Arc, Weak};

use fs_messenger_core::Encryption;
use fs_messenger_guard::Gate;
use fs_store::ObjectId;

use super::history::History;
use super::live::LiveFeed;
use super::registry::Registry;
use crate::config::ModelConfig;
use crate::error::AiError;
use crate::provider::Host;
use crate::redact::Redactor;
use crate::wire::{Dialect, Transport};

/// One provider's parts.
pub(crate) struct Shared {
    pub(crate) config: ModelConfig,
    pub(crate) account: ObjectId,
    pub(crate) host: Weak<dyn Host>,
    pub(crate) gate: Arc<dyn Gate>,
    pub(crate) dialect: Box<dyn Dialect>,
    pub(crate) transport: Transport,
    pub(crate) redactor: Redactor,
    pub(crate) live: LiveFeed,
    pub(crate) registry: Registry,
    pub(crate) history: History,
}

impl Shared {
    /// What is true of the way to this model, and never more: no value of `Encryption` says "stays on
    /// this device", and none of the end-to-end ones can be true, because a model must read the text.
    pub(crate) fn encryption(&self) -> Encryption {
        if self.config.endpoint.uses_tls() {
            Encryption::TransportOnly
        } else {
            Encryption::None
        }
    }

    pub(crate) fn host(&self) -> Result<Arc<dyn Host>, AiError> {
        self.host
            .upgrade()
            .ok_or_else(|| AiError::Host("the messenger has closed".into()))
    }
}
