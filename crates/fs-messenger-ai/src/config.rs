//! [`ModelConfig`]: which model, where, as whom.
//!
//! Pattern: Value object. The model name is configuration: there is no default for it, and `new` refuses
//! an empty one.

use crate::endpoint::Endpoint;
use crate::error::AiError;
use crate::limits::Limits;
use crate::secret::ApiKey;
use crate::tls::TlsTrust;

/// A tool a model may ask for. Shown, never run: nothing in this crate runs a tool. It comes from the
/// program, never from a model or a message.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOffer {
    /// Name.
    pub name: String,
    /// What it is for, as the model reads it.
    pub description: String,
    /// JSON Schema of its input.
    pub input_schema: serde_json::Value,
}

/// One model behind one account.
#[derive(Clone, Debug)]
pub struct ModelConfig {
    /// The participant's profile, as the person invited it. This is the sender of its answers, and
    /// the only recipient this provider takes.
    pub profile: String,
    /// The model's name on that server.
    pub model: String,
    /// Where the server is.
    pub endpoint: Endpoint,
    /// The credential for that origin, if it needs one.
    pub api_key: Option<ApiKey>,
    /// Whose certificates count.
    pub trust: TlsTrust,
    /// An instruction of the program, sent apart from every message. Never built from a message.
    pub system: Option<String>,
    /// The largest answer asked for, in tokens.
    pub max_tokens: u32,
    /// Ask for reasoning where the API needs asking. Anthropic: `thinking`, which some models refuse.
    pub ask_reasoning: bool,
    /// What the model may ask for. Empty by default; the requests are only shown.
    pub tools: Vec<ToolOffer>,
    /// What a server can make a turn cost.
    pub limits: Limits,
}

impl ModelConfig {
    /// A configuration with the given names. The rest can be set on the fields.
    pub fn new(
        profile: impl Into<String>,
        model: impl Into<String>,
        endpoint: Endpoint,
    ) -> Result<Self, AiError> {
        let (profile, model) = (profile.into(), model.into());
        if profile.trim().is_empty() || model.trim().is_empty() {
            return Err(AiError::Config(
                "a model needs a profile and a name; there is no default".into(),
            ));
        }
        Ok(Self {
            profile,
            model,
            endpoint,
            api_key: None,
            trust: TlsTrust::default(),
            system: None,
            max_tokens: 16_000,
            ask_reasoning: false,
            tools: vec![],
            limits: Limits::default(),
        })
    }
}
