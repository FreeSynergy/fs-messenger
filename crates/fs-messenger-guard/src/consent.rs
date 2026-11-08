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
    /// Someone withdrew, or refused, after the entry. Sticky: the AI must go, and only a fresh
    /// agreement ([`Consent::restart`]) can bring it back.
    #[serde(default)]
    withdrawn: bool,
    /// A person came after the entry and has not agreed yet: what is sent meanwhile is not for the AI.
    #[serde(default)]
    gap: bool,
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
            gap: false,
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
            if says == Say::Refuses && self.entered_at.is_some() {
                self.withdrawn = true;
            }
        }
        self.outcome()
    }

    /// As [`Consent::answer`], at `now`: when the last open answer of a latecomer comes in, the AI reads
    /// on from `now`, not from its old entry, so nothing sent in the gap is handed to it afterwards.
    pub fn answer_at(&mut self, person: &str, says: Say, now: u64) -> Outcome {
        let outcome = self.answer(person, says);
        if outcome == Outcome::Admitted && self.gap {
            self.gap = false;
            self.entered_at = Some(now);
        }
        outcome
    }

    /// Start again after the AI had to leave: everybody present is asked anew, and nothing of the old
    /// entry counts.
    pub fn restart(&mut self, present: &[&str]) -> Outcome {
        self.says = present
            .iter()
            .map(|p| ((*p).to_owned(), Say::Open))
            .collect();
        self.entered_at = None;
        self.withdrawn = false;
        self.gap = false;
        self.outcome()
    }

    /// Add people who are present now and not yet asked (they count as open), and take over that the
    /// AI sits in a foreign program. A later `open` of the same pair changes nothing else.
    pub fn refresh(&mut self, present: &[&str], foreign: bool) -> Outcome {
        for p in present {
            if !self.says.contains_key(*p) {
                self.arrives(p);
            }
        }
        self.foreign |= foreign;
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
        if !self.says.contains_key(person) {
            self.says.insert(person.to_owned(), Say::Open);
            // From now until the newcomer answers the AI reads nothing (`history` is `None` while the
            // outcome is `Waiting`), and what is sent meanwhile stays closed to it (`answer_at`).
            if self.entered_at.is_some() {
                self.gap = true;
            }
        }
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
    /// Messages sent after this second (the second of the entry itself is left out: with whole seconds
    /// one cannot tell which side of the entry a message in that second was).
    From(u64),
}

impl History {
    /// Whether a message sent at `sent_at` is for the AI.
    #[must_use]
    pub fn allows(self, sent_at: u64) -> bool {
        matches!(self, Self::From(from) if sent_at > from)
    }
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
