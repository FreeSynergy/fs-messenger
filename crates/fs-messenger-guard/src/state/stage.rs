//! Where one call stands: Question -> Approved -> Running -> Settled, or settled from any of them.
//!
//! Pattern: State: enum states, with what each shows. `Settled` is final. An unrecognised stage reads as
//! settled and refused: it can be neither answered nor used. The options are built here every time,
//! from the risk, and never read back from anywhere.

use fs_messenger_core::{
    ToolCall, ToolCallChoice, ToolCallKind, ToolCallOption, ToolCallOutcome, ToolCallRisk,
    ToolCallState,
};
use serde::{Deserialize, Serialize};

use super::doc::CallRecord;
use crate::digest::unhex;
use crate::risk::offers_allow_class;
use crate::table::{CapabilityRule, capability_rule};

/// One call's stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CallStage {
    Question,
    /// A person said `allowOnce`; not yet used.
    Approved,
    Running,
    Done,
    Failed,
    Cancelled,
    #[serde(other)]
    Refused,
}

impl CallStage {
    pub(crate) fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Cancelled | Self::Refused
        )
    }

    fn outcome(self) -> ToolCallOutcome {
        match self {
            Self::Question | Self::Approved | Self::Running => ToolCallOutcome::Open,
            Self::Done => ToolCallOutcome::Done,
            Self::Failed => ToolCallOutcome::Failed,
            Self::Cancelled => ToolCallOutcome::Cancelled,
            Self::Refused => ToolCallOutcome::Refused,
        }
    }

    fn state(self) -> ToolCallState {
        match self {
            Self::Question => ToolCallState::Question,
            Self::Approved | Self::Running => ToolCallState::Working,
            _ => ToolCallState::Settled,
        }
    }
}

/// The risk of a call, derived. No input is `unknown`, whatever the capability.
pub(crate) fn derived_risk(capability: &str, has_input: bool) -> ToolCallRisk {
    match capability_rule(capability) {
        CapabilityRule::Risk(risk) if has_input => risk,
        _ => ToolCallRisk::Unknown,
    }
}

/// What a question offers. There is deliberately no "always".
pub(crate) fn options_for(risk: ToolCallRisk, has_input: bool) -> Vec<ToolCallOption> {
    let option = |id: &str, label: &str, choice| ToolCallOption {
        id: id.to_string(),
        label: label.to_string(),
        choice,
    };
    let mut options = vec![
        option("refuse", "Refuse", ToolCallChoice::Refuse),
        option("allow-once", "Allow once", ToolCallChoice::AllowOnce),
    ];
    if offers_allow_class(risk, has_input) {
        options.push(option(
            "allow-class",
            "Allow this class for this participant",
            ToolCallChoice::AllowClass,
        ));
    }
    options
}

impl CallRecord {
    /// A marked read is `readsSensitive` here, which the contract does not have yet: it shows as
    /// `unknown`, which asks every time and offers no release.
    pub(crate) fn risk(&self) -> ToolCallRisk {
        if self.sensitive {
            return ToolCallRisk::Unknown;
        }
        derived_risk(&self.capability, self.has_input)
    }

    /// As shown in the messenger. `detail` is known only when the arguments are at hand.
    pub(crate) fn shown(&self, detail: &str) -> ToolCall {
        let question = self.stage == CallStage::Question;
        ToolCall {
            id: self.id.clone(),
            name: self.capability.clone(),
            kind: ToolCallKind::Unknown,
            state: self.stage.state(),
            summary: self.goes_to.clone(),
            risk: self.risk(),
            outcome: self.stage.outcome(),
            detail: detail.to_string(),
            result: String::new(),
            options: if question {
                options_for(self.risk(), self.has_input)
            } else {
                vec![]
            },
            arguments_digest: unhex(&self.digest).unwrap_or_default(),
            asked_of: self.asked_of.clone(),
            revision: self.revision,
        }
    }
}
