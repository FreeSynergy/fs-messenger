//! An AI in a conversation: the people must agree first (ADR 0054, "KIs in Unterhaltungen", 2026-11-06).
//!
//! Pattern: State machine over plain data, with no store and no clock of its own; the engine owns one
//! `Consent` per (conversation, AI) and writes it whole. Every transition is a method that returns the new
//! `Outcome`, so the rule lives in one place and is tested without a messenger.
//!
//! The rules, word for word:
//! 1. The notice comes **before** the AI enters (`Consent::open`): what the AI is, where its text goes.
//! 2. **All** present people must agree. One refusal keeps the AI out; an invitation right does not
//!    replace the agreement (the one difference to a person).
//! 3. A person who comes **later** must accept the AI too (`arrives`); else the person stays out, and the AI
//!    stays in only as long as nobody who is present refused.
//! 4. Whoever **withdraws** an agreement (`withdraw`) makes the AI leave (`Outcome::MustLeave`). That works
//!    forward only: what the AI read stays read.
//! 5. **No history from before the entry** (`History`): the AI sees only what is sent after `entered_at`.
//! 6. It holds for **every** AI, the local one too. There is no exception to switch on.
//!
//! The engine cannot force a foreign Matrix program: there the room only warns and offers to leave
//! (`Outcome::WarnOnly`), see `Consent::foreign`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What one person said.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Say {
    /// Not asked yet or not answered. Counts as no.
    Open,
    /// Agrees.
    Agrees,
    /// Said no.
    Refuses,
}

/// Where the AI stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Someone has not answered; the AI is not in.
    Waiting,
    /// All present people agree; the AI may enter, from `entered_at` on.
    Admitted,
    /// At least one person said no; the AI stays out.
    Kept,
    /// The AI was in and an agreement was withdrawn, or somebody who is present refuses now: it must go.
    MustLeave,
    /// A program we do not run: the room can only warn and offer to leave.
    WarnOnly,
}

/// The agreement of the people in one conversation about one AI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    /// The AI's participant, as the guard knows it.
    pub ai: String,
    /// People present, and what each said.
    says: BTreeMap<String, Say>,
    /// Seconds since the epoch of the entry; `None` until admitted.
    entered_at: Option<u64>,
    /// The AI is in a program we cannot bind.
    foreign: bool,
    /// Someone withdrew after the entry.
    withdrawn: bool,
}

impl Consent {
    /// The notice goes out to `present` before the AI may enter. Nobody has agreed yet.
    #[must_use]
    pub fn open(ai: &str, present: &[&str]) -> Self {
        Self {
            ai: ai.to_owned(),
            says: present
                .iter()
                .map(|p| ((*p).to_owned(), Say::Open))
                .collect(),
            entered_at: None,
            foreign: false,
            withdrawn: false,
        }
    }

    /// An AI that sits in a foreign program (for example an element room): we cannot hold it out.
    #[must_use]
    pub fn foreign(ai: &str, present: &[&str]) -> Self {
        Self {
            foreign: true,
            ..Self::open(ai, present)
        }
    }

    /// Where it stands now.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        if self.foreign {
            return Outcome::WarnOnly;
        }
        let refused = self.says.values().any(|s| *s == Say::Refuses);
        if self.entered_at.is_some() && (self.withdrawn || refused) {
            return Outcome::MustLeave;
        }
        if refused {
            return Outcome::Kept;
        }
        if !self.says.is_empty() && self.says.values().all(|s| *s == Say::Agrees) {
            Outcome::Admitted
        } else {
            Outcome::Waiting
        }
    }

    /// One present person answers. A person who is not present cannot answer; the answer is ignored.
    pub fn answer(&mut self, person: &str, says: Say) -> Outcome {
        if let Some(slot) = self.says.get_mut(person) {
            *slot = says;
        }
        self.outcome()
    }

    /// The AI enters at `now`, only when everybody agrees. Nothing sent before `now` is for it.
    pub fn enter(&mut self, now: u64) -> Outcome {
        if self.outcome() == Outcome::Admitted && self.entered_at.is_none() {
            self.entered_at = Some(now);
        }
        self.outcome()
    }

    /// A person comes later: asked at once, and an open answer counts as no until given. If the AI is in,
    /// it is asked about before the person reads on: the answer decides whether the AI may stay.
    pub fn arrives(&mut self, person: &str) -> Outcome {
        self.says.entry(person.to_owned()).or_insert(Say::Open);
        // An open answer of a newcomer does not throw the AI out: only a refusal does. Waiting for the
        // answer, the newcomer stays out of the conversation, not the AI (rule 3, `holds_out`).
        self.outcome()
    }

    /// A person leaves the conversation: their answer no longer counts.
    pub fn leaves(&mut self, person: &str) -> Outcome {
        self.says.remove(person);
        self.outcome()
    }

    /// The agreement is withdrawn. After the entry the AI must go.
    pub fn withdraw(&mut self, person: &str) -> Outcome {
        if let Some(slot) = self.says.get_mut(person) {
            *slot = Say::Refuses;
        }
        if self.entered_at.is_some() {
            self.withdrawn = true;
        }
        self.outcome()
    }

    /// May the AI see something sent at `sent_at`? Only after its entry, and not when it must go.
    #[must_use]
    pub fn history(&self) -> History {
        match (self.outcome(), self.entered_at) {
            (Outcome::Admitted, Some(from)) => History::From(from),
            _ => History::None,
        }
    }

    /// Whether this newcomer is held out of the conversation until they answer: in a room with an AI in,
    /// an open answer of a person means "not yet".
    #[must_use]
    pub fn holds_out(&self, person: &str) -> bool {
        self.entered_at.is_some() && self.says.get(person).is_some_and(|s| *s == Say::Open)
    }
}

/// What of the past the AI may see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum History {
    /// Nothing: not in, or must go.
    None,
    /// Messages sent at this second or later.
    From(u64),
}

impl History {
    /// Whether a message sent at `sent_at` is for the AI.
    #[must_use]
    pub fn allows(self, sent_at: u64) -> bool {
        matches!(self, Self::From(from) if sent_at >= from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AI: &str = "ai:claude";

    fn room() -> Consent {
        Consent::open(AI, &["anna", "ben"])
    }

    #[test]
    fn nobody_answered_the_ai_waits() {
        assert_eq!(room().outcome(), Outcome::Waiting);
        assert_eq!(room().history(), History::None);
    }

    #[test]
    fn all_must_agree_one_agreement_is_not_enough() {
        let mut c = room();
        assert_eq!(c.answer("anna", Say::Agrees), Outcome::Waiting);
        assert_eq!(c.answer("ben", Say::Agrees), Outcome::Admitted);
    }

    #[test]
    fn one_no_keeps_the_ai_out_whatever_the_others_say() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        assert_eq!(c.answer("ben", Say::Refuses), Outcome::Kept);
        assert_eq!(c.enter(10), Outcome::Kept);
        assert_eq!(c.history(), History::None);
    }

    #[test]
    fn nothing_sent_before_the_entry_is_for_the_ai() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Agrees);
        c.enter(100);
        let h = c.history();
        assert!(!h.allows(99), "a message from before");
        assert!(h.allows(100));
        assert!(h.allows(500));
    }

    #[test]
    fn it_cannot_enter_before_everyone_agreed() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        assert_eq!(c.enter(5), Outcome::Waiting);
        assert_eq!(c.history(), History::None);
    }

    #[test]
    fn withdrawing_after_the_entry_makes_the_ai_leave_and_ends_its_history() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Agrees);
        c.enter(100);
        assert_eq!(c.withdraw("ben"), Outcome::MustLeave);
        assert_eq!(c.history(), History::None);
        assert!(!c.history().allows(200));
    }

    #[test]
    fn withdrawing_before_the_entry_just_keeps_it_out() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Agrees);
        assert_eq!(c.withdraw("ben"), Outcome::Kept);
    }

    #[test]
    fn a_newcomer_is_held_out_until_they_answer_and_the_ai_stays_meanwhile() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Agrees);
        c.enter(100);
        c.arrives("cleo");
        assert!(c.holds_out("cleo"), "not in the conversation yet");
        assert!(!c.holds_out("anna"));
        // Waiting for her: the AI is not thrown out for a missing answer.
        assert_ne!(c.outcome(), Outcome::MustLeave);
        // She refuses: the AI must go, or she stays out; the conversation decides, the rule says leave.
        assert_eq!(c.answer("cleo", Say::Refuses), Outcome::MustLeave);
    }

    #[test]
    fn a_newcomer_who_accepts_completes_the_agreement() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Agrees);
        c.enter(100);
        c.arrives("cleo");
        assert_eq!(c.answer("cleo", Say::Agrees), Outcome::Admitted);
        assert!(!c.holds_out("cleo"));
    }

    #[test]
    fn someone_who_left_no_longer_counts() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        c.answer("ben", Say::Refuses);
        assert_eq!(c.leaves("ben"), Outcome::Admitted);
    }

    #[test]
    fn an_empty_room_admits_nobody() {
        assert_eq!(Consent::open(AI, &[]).outcome(), Outcome::Waiting);
    }

    #[test]
    fn a_foreign_program_is_warned_about_never_held_out() {
        let mut c = Consent::foreign(AI, &["anna"]);
        c.answer("anna", Say::Agrees);
        assert_eq!(c.outcome(), Outcome::WarnOnly);
        assert_eq!(c.history(), History::None, "we promise nothing there");
    }

    #[test]
    fn an_answer_from_a_stranger_changes_nothing() {
        let mut c = room();
        c.answer("mallory", Say::Agrees);
        c.answer("anna", Say::Agrees);
        assert_eq!(c.outcome(), Outcome::Waiting);
    }

    #[test]
    fn it_survives_a_round_trip_through_json() {
        let mut c = room();
        c.answer("anna", Say::Agrees);
        let back: Consent = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
