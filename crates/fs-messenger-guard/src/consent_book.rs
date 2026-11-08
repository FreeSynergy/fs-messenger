//! [`ConsentBook`]: the agreements of ADR 0054 kept in the store, one per (conversation, AI).
//!
//! Pattern: Repository around the pure [`Consent`] — read, change, write under one lock, so two answers
//! cannot overwrite each other. The rules stay in `consent.rs` and are tested without a store; this
//! file keeps them across restarts, hands out the notice, and answers what an AI may read. A conversation
//! the engine does not run (a foreign Matrix room) is opened with `foreign = true` and only ever warns.

use std::sync::Arc;

use fs_objects::{Envelope, ObjectId, Query, Store, StoredObject};
use tokio::sync::Mutex;

use crate::clock::Clock;
use crate::consent::{Consent, History, Outcome, Say};
use crate::error::GuardError;
use crate::words::{CONSENT_NOTICE, DESTINATION_UNKNOWN, FOREIGN_WARNING};

const TYPE: &str = "freesynergy.net:AiConsent";
const CONTEXT: &str = "messenger-guard";

/// The agreements of a messenger.
pub struct ConsentBook {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    actor: String,
    turn: Mutex<()>,
}

fn not_persisted(e: &fs_objects::Error) -> GuardError {
    GuardError::NotPersisted(e.to_string())
}

impl ConsentBook {
    /// A book over `store`; `actor` is who the store records as the writer.
    pub fn new(store: Arc<dyn Store>, clock: Arc<dyn Clock>, actor: impl Into<String>) -> Self {
        Self {
            store,
            clock,
            actor: actor.into(),
            turn: Mutex::new(()),
        }
    }

    async fn find(
        &self,
        conversation: ObjectId,
        ai: &str,
    ) -> Result<Option<StoredObject>, GuardError> {
        let q = Query::new()
            .of_type(TYPE)
            .in_context(CONTEXT)
            .with_index("conversation", conversation.to_string())
            .with_index("ai", ai.to_string());
        Ok(self
            .store
            .query(&q)
            .await
            .map_err(|e| not_persisted(&e))?
            .into_iter()
            .next())
    }

    async fn save(
        &self,
        conversation: ObjectId,
        ai: &str,
        consent: &Consent,
        existing: Option<StoredObject>,
    ) -> Result<(), GuardError> {
        let value =
            serde_json::to_value(consent).map_err(|e| GuardError::NotPersisted(e.to_string()))?;
        let object = if let Some(mut o) = existing {
            o.content = value;
            o
        } else {
            let mut env = Envelope::new(TYPE, "ai consent");
            env.context = Some(CONTEXT.into());
            StoredObject::new(env, "application/json", value)
                .with_index("conversation", conversation.to_string())
                .with_index("ai", ai.to_string())
        };
        self.store
            .put(object, &self.actor)
            .await
            .map_err(|e| not_persisted(&e))?;
        Ok(())
    }

    async fn change(
        &self,
        conversation: ObjectId,
        ai: &str,
        f: impl FnOnce(&mut Consent, u64) -> Outcome,
    ) -> Result<Outcome, GuardError> {
        let _turn = self.turn.lock().await;
        let Some(found) = self.find(conversation, ai).await? else {
            return Err(GuardError::Refused(crate::error::Refusal::UnknownCall));
        };
        let mut consent: Consent = serde_json::from_value(found.content.clone())
            .map_err(|e| GuardError::NotPersisted(e.to_string()))?;
        let now = u64::try_from(self.clock.now().timestamp()).unwrap_or(0);
        let outcome = f(&mut consent, now);
        self.save(conversation, ai, &consent, Some(found)).await?;
        Ok(outcome)
    }

    /// The notice goes out to `present` before the AI may enter. A second call for the same pair keeps
    /// the answers given so far. `foreign`: the AI sits in a program we do not run.
    pub async fn open(
        &self,
        conversation: ObjectId,
        ai: &str,
        present: &[&str],
        foreign: bool,
    ) -> Result<Outcome, GuardError> {
        let _turn = self.turn.lock().await;
        if let Some(found) = self.find(conversation, ai).await? {
            let consent: Consent = serde_json::from_value(found.content)
                .map_err(|e| GuardError::NotPersisted(e.to_string()))?;
            return Ok(consent.outcome());
        }
        let consent = if foreign {
            Consent::foreign(ai, present)
        } else {
            Consent::open(ai, present)
        };
        self.save(conversation, ai, &consent, None).await?;
        Ok(consent.outcome())
    }

    /// A present person answers.
    pub async fn answer(
        &self,
        conversation: ObjectId,
        ai: &str,
        person: &str,
        says: Say,
    ) -> Result<Outcome, GuardError> {
        self.change(conversation, ai, |c, _| c.answer(person, says))
            .await
    }

    /// The AI enters, if everybody agrees; from now on it may read what is sent.
    pub async fn enter(&self, conversation: ObjectId, ai: &str) -> Result<Outcome, GuardError> {
        self.change(conversation, ai, Consent::enter).await
    }

    /// A person comes later and is asked at once.
    pub async fn arrives(
        &self,
        conversation: ObjectId,
        ai: &str,
        person: &str,
    ) -> Result<Outcome, GuardError> {
        self.change(conversation, ai, |c, _| c.arrives(person))
            .await
    }

    /// A person leaves the conversation; their answer no longer counts.
    pub async fn leaves(
        &self,
        conversation: ObjectId,
        ai: &str,
        person: &str,
    ) -> Result<Outcome, GuardError> {
        self.change(conversation, ai, |c, _| c.leaves(person)).await
    }

    /// A person takes the agreement back. After the entry the AI must go.
    pub async fn withdraw(
        &self,
        conversation: ObjectId,
        ai: &str,
        person: &str,
    ) -> Result<Outcome, GuardError> {
        self.change(conversation, ai, |c, _| c.withdraw(person))
            .await
    }

    /// Where the AI stands in this conversation; `None` if no notice was ever sent for it.
    pub async fn outcome(
        &self,
        conversation: ObjectId,
        ai: &str,
    ) -> Result<Option<Outcome>, GuardError> {
        Ok(self.read(conversation, ai).await?.map(|c| c.outcome()))
    }

    /// What of the past the AI may read. **Nothing** where no notice exists: an AI that never went
    /// through the agreement gets no history.
    pub async fn history(&self, conversation: ObjectId, ai: &str) -> Result<History, GuardError> {
        Ok(self
            .read(conversation, ai)
            .await?
            .map_or(History::None, |c| c.history()))
    }

    /// Whether a message sent at `sent_at` (seconds since the epoch) may go to the AI.
    pub async fn may_read(
        &self,
        conversation: ObjectId,
        ai: &str,
        sent_at: u64,
    ) -> Result<bool, GuardError> {
        Ok(self.history(conversation, ai).await?.allows(sent_at))
    }

    /// Whether a newcomer is held out of the conversation until they answer about the AI.
    pub async fn holds_out(
        &self,
        conversation: ObjectId,
        ai: &str,
        person: &str,
    ) -> Result<bool, GuardError> {
        Ok(self
            .read(conversation, ai)
            .await?
            .is_some_and(|c| c.holds_out(person)))
    }

    async fn read(&self, conversation: ObjectId, ai: &str) -> Result<Option<Consent>, GuardError> {
        self.find(conversation, ai)
            .await?
            .map(|o| serde_json::from_value(o.content))
            .transpose()
            .map_err(|e| GuardError::NotPersisted(e.to_string()))
    }

    /// The notice a person reads before the AI enters, or the warning of a room we do not run.
    /// `destination` is where the AI's text goes ("Anthropic"); `None` says "unbekannt".
    #[must_use]
    pub fn notice(ai: &str, destination: Option<&str>, foreign: bool) -> String {
        let text = if foreign {
            FOREIGN_WARNING
        } else {
            CONSENT_NOTICE
        };
        text.replace("{ai}", ai)
            .replace("{destination}", destination.unwrap_or(DESTINATION_UNKNOWN))
    }
}
