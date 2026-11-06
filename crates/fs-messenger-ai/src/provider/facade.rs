//! [`ModelProvider`]: a model, as one account's provider.
//!
//! Pattern: Facade + Strategy: the core sees `Provider`; behind it one flow and one of two dialects.
//! `send` answers as soon as the turn has started, because the core learns the conversation's alias
//! from that answer and a stop needs it. Nothing here runs a tool.

use std::sync::Arc;

use async_trait::async_trait;
use fs_interfaces::provider::Capabilities;
use fs_messenger_core::{
    EncryptionPlan, Event, ExternalId, Outgoing, Provider, Result as CoreResult, SendReceipt,
    TurnScope,
};
use fs_store::ObjectId;
use tokio::sync::broadcast;

use super::choice::{Choices, RemoteChoice};
use super::host::{ProviderLink, agent_account, conversation_of};
use super::plan::{deliverable, honest_plan};
use crate::config::ModelConfig;
use crate::endpoint::DataPath;
use crate::error::AiError;
use crate::redact::Redactor;
use crate::turn::{
    CONVERSATION_SCHEME, History, LiveFeed, LiveTurn, Registry, Shared, TurnInput, start,
};
use crate::wire::{Anthropic, Dialect, LlamaCpp, Transport};

const KIND: &str = "agent";
const MESSAGE_SCHEME: &str = "model-message";

/// One model behind the core's `Provider` trait.
pub struct ModelProvider {
    shared: Arc<Shared>,
    choices: Choices,
}

impl ModelProvider {
    /// A llama.cpp server. A key only if the server was started with one.
    pub async fn llama_cpp(config: ModelConfig, link: ProviderLink) -> Result<Self, AiError> {
        Self::attached(config, link, Box::new(LlamaCpp)).await
    }

    /// The Anthropic Messages API, or something at another origin that speaks it. Needs a key.
    pub async fn anthropic(config: ModelConfig, link: ProviderLink) -> Result<Self, AiError> {
        Self::attached(config, link, Box::new(Anthropic)).await
    }

    async fn attached(
        config: ModelConfig,
        link: ProviderLink,
        dialect: Box<dyn Dialect>,
    ) -> Result<Self, AiError> {
        // Refused now rather than at the first turn.
        dialect.headers(config.api_key.as_ref())?;
        agent_account(&link).await?;
        let shared = Shared {
            transport: Transport::new(&config)?,
            redactor: Redactor::new(config.api_key.clone()),
            live: LiveFeed::new(),
            registry: Registry::default(),
            history: History::new(&config.limits),
            account: link.account,
            host: link.host,
            gate: link.gate,
            dialect,
            config,
        };
        Ok(Self {
            shared: Arc::new(shared),
            choices: Choices::default(),
        })
    }

    /// Where a person's text goes. A frontend shows this before the person sends.
    pub fn data_path(&self) -> DataPath {
        self.shared.config.endpoint.data_path()
    }

    /// A person chose to send this text to this remote model. Without it a remote model gets nothing.
    pub fn choose_remote(&self, choice: RemoteChoice) {
        self.choices.add(choice);
    }

    /// The live side of the turns: `created`, `text`, `state`, as the contract describes them. Not the
    /// messenger's subscription, whose message ids these are not. See [`LiveTurn`].
    pub fn live_events(&self) -> broadcast::Receiver<Event> {
        self.shared.live.subscribe()
    }

    /// A running turn, or one of the last that ended, by the id its events use.
    pub fn live_turn(&self, message: ObjectId) -> Option<LiveTurn> {
        self.shared.live.turn(message)
    }

    /// Whether a turn runs in a conversation this provider calls by this alias.
    pub fn is_running(&self, conversation: &ExternalId) -> bool {
        conversation.scheme == CONVERSATION_SCHEME
            && self.shared.registry.is_running(&conversation.id)
    }

    async fn begin(&self, message: &Outgoing) -> Result<SendReceipt, AiError> {
        let shared = &self.shared;
        deliverable(message, &shared.config.profile)?;
        let alias = message
            .conversation()
            .iter()
            .find(|c| c.scheme == CONVERSATION_SCHEME)
            .cloned()
            .unwrap_or_else(|| ExternalId::new(CONVERSATION_SCHEME, ObjectId::new().to_string()));
        // The place first, the guard after: a stop that finds no turn yet has set the guard's latch before
        // it looked, so this asks too late to pass.
        let ticket = shared.registry.begin(&alias.id)?;
        shared
            .gate
            .admit_message(&shared.config.profile)
            .await
            .map_err(|e| AiError::NotAdmitted(e.to_string()))?;
        if matches!(self.data_path(), DataPath::LeavesDevice { .. })
            && !self.choices.take(message.text())
        {
            return Err(AiError::RemoteNotChosen);
        }
        let host = shared.host()?;
        let conversation = conversation_of(host.as_ref(), message.id()).await?;
        let parent_alias = ExternalId::new(MESSAGE_SCHEME, message.id().to_string());
        let input = TurnInput {
            conversation,
            conversation_alias: alias.clone(),
            person_message: message.id(),
            parent_alias: parent_alias.clone(),
            turn_id: ObjectId::new().to_string(),
            text: message.text().to_string(),
            lines: shared.history.with_next(&alias.id, message.text()),
        };
        start(shared.clone(), input, ticket);
        Ok(SendReceipt {
            aliases: vec![parent_alias],
            conversation: Some(alias),
            encryption: shared.encryption(),
        })
    }
}

#[async_trait]
impl Provider for ModelProvider {
    fn kind(&self) -> CoreResult<String> {
        Ok(KIND.to_string())
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new("fs-messenger-ai");
        for id in [
            "provider.kind",
            "message.send",
            "encryption.plan",
            "turn.stop",
        ] {
            caps.supported(id);
        }
        caps.unsupported("message.fetch", "answers come in as a turn ends");
        caps
    }

    /// Never end to end: a model must read the text. So the core refuses this, every time, until the
    /// owner decides otherwise. See `DESIGN.md`.
    async fn encryption_plan(&self, recipients: &[String]) -> CoreResult<EncryptionPlan> {
        Ok(honest_plan(self.shared.encryption(), recipients))
    }

    /// Only reached if a plan passed the core, and no honest plan of this provider does. It reports what
    /// is true whatever `message.encryption()` says.
    async fn send(&self, message: &Outgoing) -> CoreResult<SendReceipt> {
        Ok(self.begin(message).await?)
    }

    async fn stop_turns(&self, scope: &TurnScope) -> CoreResult<()> {
        Ok(self.shared.registry.stop(scope).await?)
    }
}
