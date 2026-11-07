//! What a release covers: participant, class and program together (ADR 0054, "Ohne Befugnis nichts").
//!
//! Pattern: Specification: pure predicates over the stored grants, with no state of their own and no
//! side effect. A grant counts only if it names a program and where the text goes, and only for a class
//! that can be released at all, whatever a record says. Nothing is released by default.

use fs_messenger_core::ToolCallRisk;

use super::doc::GuardDoc;
use crate::model::AiGrant;
use crate::risk::can_be_released;

/// Whether this grant is a release of the participant in the program.
fn counts_for(grant: &AiGrant, profile: &str, program: &str) -> bool {
    grant.profile == profile
        && !program.is_empty()
        && grant.program == program
        && !grant.goes_to.is_empty()
        && can_be_released(grant.risk)
}

impl GuardDoc {
    /// Is this class released for this participant in this program?
    pub(crate) fn is_released(&self, profile: &str, risk: ToolCallRisk, program: &str) -> bool {
        can_be_released(risk)
            && self
                .grants
                .iter()
                .any(|g| g.risk == risk && counts_for(g, profile, program))
    }

    /// Is anything, of any class, released for this participant in this program? Only then may a call
    /// that always asks reach a person.
    pub(crate) fn has_release_in(&self, profile: &str, program: &str) -> bool {
        self.grants.iter().any(|g| counts_for(g, profile, program))
    }

    /// Where the text goes, in the words of the person who released, for a question to show.
    pub(crate) fn destinations(&self, profile: &str, program: &str) -> String {
        let mut words: Vec<&str> = vec![];
        for grant in self
            .grants
            .iter()
            .filter(|g| counts_for(g, profile, program))
        {
            if !words.contains(&grant.goes_to.as_str()) {
                words.push(&grant.goes_to);
            }
        }
        words.join("; ")
    }
}
