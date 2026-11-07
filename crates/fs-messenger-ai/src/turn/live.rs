//! The live side of a turn: the core's own [`Event`] values, on a channel of this crate.
//!
//! Pattern: Observer. The contract wants `created`, `text`, then `state` on the messenger's subscription,
//! but the core offers a provider no way onto it and no way to store a message as `sending`, so this
//! feed exists beside it, until the core has one (`DESIGN.md`, "Wanted"). Its message id is therefore
//! not the stored message's id: [`LiveTurn::stored`] and `job_ref` link the two. A listener that lags
//! reads the text so far from [`LiveTurn`], as the contract says for an offset above what it holds.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use fs_messenger_core::{
    DeliveryState, Encryption, Event, Kind, MessageSummary, SenderKind, SignatureCheck,
    StateChange, TextChunk, TextPart,
};
use fs_objects::{Envelope, ObjectId};
use tokio::sync::broadcast;

use super::failure::TurnFailure;

const CHANNEL: usize = 1024;
const KEPT_TURNS: usize = 32;

/// A turn as it stands: running, or one of the last that ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTurn {
    /// The id its events use; not the id of the stored message.
    pub message: ObjectId,
    /// Its conversation.
    pub conversation: ObjectId,
    /// `turn:<id>`, also the stored message's `job_ref`.
    pub job_ref: String,
    /// `sending`, then `sent` or `failed`.
    pub state: DeliveryState,
    /// The answer so far.
    pub body: String,
    /// The reasoning so far, apart from the answer.
    pub reasoning: String,
    /// Why it failed.
    pub failure: Option<TurnFailure>,
    /// The stored message, once there is one.
    pub stored: Option<ObjectId>,
}

/// What opens a turn.
pub(crate) struct LiveOpening<'a> {
    pub(crate) conversation: ObjectId,
    pub(crate) job_ref: &'a str,
    pub(crate) profile: &'a str,
    pub(crate) encryption: Encryption,
}

/// The channel and the turns.
pub(crate) struct LiveFeed {
    events: broadcast::Sender<Event>,
    turns: Mutex<VecDeque<LiveTurn>>,
}

impl LiveFeed {
    pub(crate) fn new() -> Self {
        Self {
            events: broadcast::channel(CHANNEL).0,
            turns: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn send(&self, event: Event) {
        // Nobody listening is no failure.
        let _ = self.events.send(event);
    }

    pub(crate) fn turn(&self, id: ObjectId) -> Option<LiveTurn> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.iter().find(|t| t.message == id).cloned()
    }

    fn with(&self, id: ObjectId, change: impl FnOnce(&mut LiveTurn)) {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.iter_mut().find(|t| t.message == id) {
            change(turn);
        }
    }

    /// `created` comes first. `sender_kind` stays `unknown` here: only the guard's lookup says what a
    /// sender is, and the core asks it when it stores the answer.
    pub(crate) fn open(&self, opening: &LiveOpening<'_>) -> ObjectId {
        let mut envelope = Envelope::new("Note", opening.profile);
        envelope.attributed_to = Some(opening.profile.to_string());
        envelope.context = Some(opening.conversation.to_string());
        let message = envelope.id;
        let summary = MessageSummary {
            envelope,
            kind: Kind::Chat,
            encryption: opening.encryption,
            state: DeliveryState::Sending,
            unread: true,
            flagged: false,
            preview: String::new(),
            sender_kind: SenderKind::Unknown,
            job_ref: opening.job_ref.to_string(),
            arrival: 0,
            undecrypted: false,
            signature: SignatureCheck::default(),
            too_large: 0,
        };
        {
            let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            while turns.len() >= KEPT_TURNS {
                let Some(old) = turns.iter().position(|t| t.state != DeliveryState::Sending) else {
                    break;
                };
                turns.remove(old);
            }
            turns.push_back(LiveTurn {
                message,
                conversation: opening.conversation,
                job_ref: opening.job_ref.to_string(),
                state: DeliveryState::Sending,
                body: String::new(),
                reasoning: String::new(),
                failure: None,
                stored: None,
            });
        }
        self.send(Event::Created(summary));
        message
    }

    pub(crate) fn text(&self, id: ObjectId, part: TextPart, offset: u64, text: String) {
        let mut conversation = None;
        self.with(id, |turn| {
            conversation = Some(turn.conversation);
            match part {
                TextPart::Body => turn.body.push_str(&text),
                TextPart::Reasoning => turn.reasoning.push_str(&text),
            }
        });
        if let Some(conversation) = conversation {
            self.send(Event::Text(TextChunk {
                message: id,
                part,
                offset,
                text,
                conversation,
            }));
        }
    }

    /// The stored message's id, as soon as there is one.
    pub(crate) fn stored(&self, id: ObjectId, stored: ObjectId) {
        self.with(id, |turn| turn.stored = Some(stored));
    }

    /// The state event ends it, and nothing follows.
    pub(crate) fn end(&self, id: ObjectId, failure: Option<TurnFailure>) {
        let state = if failure.is_some() {
            DeliveryState::Failed
        } else {
            DeliveryState::Sent
        };
        self.with(id, |turn| {
            turn.state = state;
            turn.failure = failure;
        });
        self.send(Event::State(StateChange { message: id, state }));
    }
}
