//! [`LocalEnrolment`]: how a device key gets in.
//!
//! Pattern: Capability object: exists only as what `GuardEngine::open` hands back, is not `Clone`, and
//! no method of `Gate`, `Guard`, `Messenger` or `Provider` leads to it. The program keeps it with its
//! local console. **Provisional: the first key.** Whoever holds this at the first start can put in the
//! owner's first key: trust on first use, on the local machine. Later keys need a proof by a key the
//! owner already has. A lost last key cannot be replaced here while the state is intact: AIs stay shut
//! out, and the rest of the messenger works. See `DESIGN.md`.

use std::sync::Arc;

use crate::digest::hex;
use crate::engine::{GuardEngine, View};
use crate::error::{GuardError, Refusal};
use crate::log::GuardLogEntry;
use crate::model::GuardProof;
use crate::signed::Method;
use crate::state::{DeviceChange, GuardDoc};

/// The local way in for device keys. Public keys only.
pub struct LocalEnrolment {
    engine: Arc<GuardEngine>,
}

fn new_identity() -> Result<String, GuardError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| GuardError::NotPersisted(e.to_string()))?;
    Ok(hex(&bytes))
}

impl LocalEnrolment {
    pub(crate) fn new(engine: Arc<GuardEngine>) -> Self {
        Self { engine }
    }

    fn check_key(&self, public_key: &[u8]) -> Result<(), Refusal> {
        if self.engine.verifier.accepts_key(public_key) {
            Ok(())
        } else {
            Err(Refusal::BadKey)
        }
    }

    fn add_owner_key(&self, doc: &mut GuardDoc, public_key: &[u8]) {
        doc.add_device(DeviceChange {
            person: &self.engine.config.owner,
            key_type: self.engine.verifier.key_type(),
            public_key: hex(public_key),
        });
    }

    async fn commit(&self, doc: &GuardDoc, entry: &GuardLogEntry) -> Result<(), GuardError> {
        self.engine
            .save(doc)
            .await
            .map_err(GuardError::NotPersisted)?;
        self.engine
            .repo
            .append(entry)
            .await
            .map_err(GuardError::DoneNotShown)
    }

    /// The owner's first key, once. The program stays stopped; `reopen` opens it. Refused if the owner
    /// has a key already, and on a damaged state, which only `recover_damaged` goes on from.
    pub async fn enrol_owner(&self, public_key: &[u8]) -> Result<(), GuardError> {
        self.check_key(public_key)?;
        let owner = self.engine.config.owner.clone();
        let _ops = self.engine.ops.lock().await;
        let mut doc = match self.engine.view().await {
            View::Missing => GuardDoc::fresh(new_identity()?),
            View::Doc(doc) => *doc,
            View::Unreadable(_) | View::Corrupt(_) => return Err(Refusal::Damaged.into()),
        };
        doc.check_device(&owner, &owner)?;
        if doc.devices.contains_key(&owner) {
            return Err(Refusal::NotEnrollable("the owner has a key; add with its proof").into());
        }
        self.add_owner_key(&mut doc, public_key);
        self.commit(&doc, &GuardLogEntry::new("enrol", &owner).about(&owner))
            .await
    }

    /// After damage: a new state with this owner key, stopped, nobody invited, nothing released, and a
    /// new engine identity, so that every proof signed before no longer fits. Only on a state that was
    /// read and is not one: never because the store did not answer.
    pub async fn recover_damaged(&self, public_key: &[u8]) -> Result<(), GuardError> {
        self.check_key(public_key)?;
        let owner = self.engine.config.owner.clone();
        let _ops = self.engine.ops.lock().await;
        if !matches!(self.engine.view().await, View::Corrupt(_)) {
            return Err(Refusal::NotEnrollable("the state is not damaged").into());
        }
        let mut doc = GuardDoc::fresh(new_identity()?);
        self.add_owner_key(&mut doc, public_key);
        self.commit(&doc, &GuardLogEntry::new("recover", &owner).about(&owner))
            .await
    }

    async fn owner_signed<F>(
        &self,
        method: Method,
        person: &str,
        public_key: &[u8],
        proof: &GuardProof,
        event: &str,
        change: F,
    ) -> Result<(), GuardError>
    where
        F: FnOnce(&mut GuardDoc) -> Result<(), Refusal> + Send,
    {
        let owner = self.engine.config.owner.clone();
        if proof.signer != owner {
            return Err(Refusal::NotPermitted.into());
        }
        let key_text = hex(public_key);
        let arguments: [&[u8]; 2] = [person.as_bytes(), key_text.as_bytes()];
        let entry = GuardLogEntry::new(event, &proof.signer).about(person);
        self.engine
            .signed_change(method, &arguments, proof, entry, change)
            .await?;
        Ok(())
    }

    /// Another key for a person, with a proof by a key of the owner. Never for an invited AI.
    pub async fn add_device(
        &self,
        person: &str,
        public_key: &[u8],
        proof: &GuardProof,
    ) -> Result<(), GuardError> {
        self.check_key(public_key)?;
        let owner = self.engine.config.owner.clone();
        let key_type = self.engine.verifier.key_type();
        let public = hex(public_key);
        self.owner_signed(
            Method::AddDevice,
            person,
            public_key,
            proof,
            "enrol",
            |doc| {
                doc.check_device(person, &owner)?;
                doc.add_device(DeviceChange {
                    person,
                    key_type,
                    public_key: public,
                });
                Ok(())
            },
        )
        .await
    }

    /// Take a key away, with a proof by a key of the owner: a lost device.
    pub async fn remove_device(
        &self,
        person: &str,
        public_key: &[u8],
        proof: &GuardProof,
    ) -> Result<(), GuardError> {
        let public = hex(public_key);
        self.owner_signed(
            Method::RemoveDevice,
            person,
            public_key,
            proof,
            "removeDevice",
            |doc| {
                if doc.remove_device(person, &public) {
                    Ok(())
                } else {
                    Err(Refusal::BadKey)
                }
            },
        )
        .await
    }
}
