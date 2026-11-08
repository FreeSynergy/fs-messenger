//! Accepted ideas 4 and 1 (`requirements.md`) kept in the store: the person's choice about read
//! receipts, and messages held for a while before they go out.
//!
//! Pattern: Repository around two pure rules — `receipts_both_ways` (Specification) and `CoolDown`
//! (State machine, `now` passed in). This file reads, applies the rule and writes, under the one write
//! lock; it decides nothing itself. A held message is a record that names a saved draft and its stage.

use fs_objects::{Envelope, ObjectId, Query, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{CONTENT_TYPE, DRAFT, SENSITIVE};
use super::handle::Inner;
use crate::cooldown::Stage;
use crate::error::{Error, Result};
use crate::receipts::ReceiptChoice;

const SETTING: &str = "freesynergy.net:Setting";
const HELD: &str = "freesynergy.net:Held";
const IDX_NAME: &str = "name";
const IDX_STAGE: &str = "stage";
const RECEIPTS: &str = "receipts";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeldContent {
    draft: ObjectId,
    stage: Stage,
    /// The message the draft became, once it went out.
    sent: Option<ObjectId>,
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Held { .. } => "held",
        Stage::SentUnread => "sent-unread",
        Stage::SentRead => "sent-read",
        Stage::Recalled => "recalled",
    }
}

impl Inner {
    /// The person's choice about read receipts; `Hidden` until they choose.
    pub(super) async fn receipt_choice(&self) -> Result<ReceiptChoice> {
        let q = Query::new().of_type(SETTING).with_index(IDX_NAME, RECEIPTS);
        Ok(self
            .store
            .query(&q)
            .await?
            .into_iter()
            .next()
            .and_then(|o| serde_json::from_value(o.content).ok())
            .unwrap_or_default())
    }

    pub(super) async fn set_receipt_choice(&self, choice: ReceiptChoice) -> Result<()> {
        let _writes = self.writes.lock().await;
        let q = Query::new().of_type(SETTING).with_index(IDX_NAME, RECEIPTS);
        let value = serde_json::to_value(choice)?;
        let object = if let Some(mut o) = self.store.query(&q).await?.into_iter().next() {
            o.content = value;
            o
        } else {
            let mut env = Envelope::new(SETTING, RECEIPTS);
            env.sensitive = vec![SENSITIVE.to_string()];
            StoredObject::new(env, CONTENT_TYPE, value).with_index(IDX_NAME, RECEIPTS)
        };
        self.put(object).await
    }

    /// Hold the saved draft until `now + wait`.
    pub(super) async fn hold(&self, draft: ObjectId, now: u64, wait: u64) -> Result<ObjectId> {
        let _writes = self.writes.lock().await;
        self.store
            .get(draft)
            .await?
            .filter(|o| o.envelope.object_type == DRAFT)
            .ok_or(Error::NotFound(draft))?;
        let stage = Stage::hold(now, wait);
        let mut env = Envelope::new(HELD, "held message");
        env.sensitive = vec![SENSITIVE.to_string()];
        let id = env.id;
        let content = HeldContent {
            draft,
            stage,
            sent: None,
        };
        let object = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(&content)?)
            .with_index(IDX_STAGE, stage_name(stage));
        self.put(object).await?;
        Ok(id)
    }

    async fn load_held(&self, id: ObjectId) -> Result<(StoredObject, HeldContent)> {
        let o = self
            .store
            .get(id)
            .await?
            .filter(|o| o.envelope.object_type == HELD)
            .ok_or(Error::NotFound(id))?;
        let c = serde_json::from_value(o.content.clone())?;
        Ok((o, c))
    }

    async fn save_held(&self, mut o: StoredObject, c: &HeldContent) -> Result<()> {
        o.content = serde_json::to_value(c)?;
        o.index
            .insert(IDX_STAGE.to_string(), vec![stage_name(c.stage).to_string()]);
        self.put(o).await
    }

    /// Where a held message stands.
    pub(super) async fn held_stage(&self, id: ObjectId) -> Result<Stage> {
        Ok(self.load_held(id).await?.1.stage)
    }

    /// The draft behind a held message that is still held and due at `now` (or `force`d now), and the
    /// stage it had. `None` if it is not held any more: nothing is sent twice.
    pub(super) async fn take_due(
        &self,
        id: ObjectId,
        now: u64,
        force: bool,
    ) -> Result<Option<(ObjectId, Stage)>> {
        let _writes = self.writes.lock().await;
        let (o, mut c) = self.load_held(id).await?;
        let next = if force {
            c.stage.send_now()
        } else {
            c.stage.release(now)
        };
        if next == c.stage || !matches!(c.stage, Stage::Held { .. }) {
            return Ok(None);
        }
        let before = c.stage;
        c.stage = next;
        self.save_held(o, &c).await?;
        Ok(Some((c.draft, before)))
    }

    /// The send failed: the message is held again, as before, and goes out at the next try.
    pub(super) async fn restore_held(&self, id: ObjectId, before: Stage) -> Result<()> {
        let _writes = self.writes.lock().await;
        let (o, mut c) = self.load_held(id).await?;
        c.stage = before;
        self.save_held(o, &c).await
    }

    /// Record the message a released draft became.
    pub(super) async fn held_sent(&self, id: ObjectId, message: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let (o, mut c) = self.load_held(id).await?;
        c.sent = Some(message);
        self.save_held(o, &c).await
    }

    /// Put a held message back: it is a draft again, never sent. Only a held message can be taken back
    /// here; one that is out cannot be called back by a provider that has no such thing.
    pub(super) async fn recall_held(&self, id: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        let (o, mut c) = self.load_held(id).await?;
        if !matches!(c.stage, Stage::Held { .. }) {
            return Err(crate::error::Error::Conflict(
                "it is not held: it went out, or was taken back; no provider here can recall a sent message".into(),
            ));
        }
        c.stage = c.stage.recall();
        self.save_held(o, &c).await
    }

    /// The ids of the held messages whose time is up at `now`.
    pub(super) async fn due_held(&self, now: u64) -> Result<Vec<ObjectId>> {
        let q = Query::new().of_type(HELD).with_index(IDX_STAGE, "held");
        let mut due = vec![];
        for o in self.store.query(&q).await? {
            if let Ok(c) = serde_json::from_value::<HeldContent>(o.content)
                && c.stage != c.stage.release(now)
            {
                due.push(o.envelope.id);
            }
        }
        Ok(due)
    }
}
