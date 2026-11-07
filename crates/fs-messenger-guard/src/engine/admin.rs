//! [`Guard`] on the engine, apart from the stop: invite, release, revoke, reopen.
//!
//! Pattern: Facade over the pure state; `signed_change` is a plain helper that fixes the order (lock, read,
//! proof, rule, copy, write, log).
//! A refusal uses up nothing and writes nothing. Reopen writes before it opens: if it cannot be
//! written, it stays shut.

use async_trait::async_trait;

use super::core::GuardEngine;
use super::marker;
use super::store_access::View;
use crate::api::Guard;
use crate::error::{GuardError, Refusal};
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::model::{AiGrant, GuardMode, GuardProof, GuardState, StopReport};
use crate::signed::Method;
use crate::state::{GuardDoc, ProofRequest};

/// The arguments of `release`: the profile, and the risk by its contract name.
pub(crate) fn risk_name(risk: fs_messenger_core::ToolCallRisk) -> &'static str {
    use fs_messenger_core::ToolCallRisk as R;
    match risk {
        R::ReadsOnly => "readsOnly",
        R::Changes => "changes",
        R::Destroys => "destroys",
        R::ActsForPerson => "actsForPerson",
        R::Unknown => "unknown",
    }
}

impl GuardEngine {
    /// `revoke` and `uninvite`: what hangs on a participant goes, and what is shown follows.
    async fn take_away(
        &self,
        method: Method,
        event: &str,
        profile: &str,
        proof: &GuardProof,
    ) -> Result<(), GuardError> {
        let owner = self.config.owner.clone();
        let (key, signer) = (id_key(profile), id_key(&proof.signer));
        let entry = GuardLogEntry::new(event, &signer).about(&key);
        let uninvite = method == Method::Uninvite;
        let mut cancelled = vec![];
        let doc = self
            .signed_change(method, &[profile.as_bytes()], proof, entry, |doc| {
                doc.check_revoke(&key, &signer, &owner)?;
                cancelled = if uninvite {
                    doc.apply_uninvite(&key)
                } else {
                    doc.apply_revoke(&key)
                };
                Ok(())
            })
            .await?;
        self.show_all(&doc, &cancelled).await
    }

    pub(crate) async fn show_all(&self, doc: &GuardDoc, ids: &[String]) -> Result<(), GuardError> {
        let mut problems = vec![];
        for id in ids {
            if let Some(call) = doc.calls.iter().find(|c| &c.id == id) {
                if let Err(why) = self.show(call, "").await {
                    problems.push(why);
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(GuardError::DoneNotShown(problems.join("; ")))
        }
    }

    /// The state to change, or why not. A fresh program has no keys yet, and damage cannot be checked.
    pub(crate) fn writable(view: View) -> Result<GuardDoc, Refusal> {
        match view {
            View::Doc(doc) => Ok(*doc),
            View::Missing => Err(Refusal::Stopped),
            View::Unreadable(_) | View::Corrupt(_) => Err(Refusal::Damaged),
        }
    }

    pub(crate) async fn signed_change<F>(
        &self,
        method: Method,
        arguments: &[&[u8]],
        proof: &GuardProof,
        entry: GuardLogEntry,
        change: F,
    ) -> Result<GuardDoc, GuardError>
    where
        F: FnOnce(&mut GuardDoc) -> Result<(), Refusal> + Send,
    {
        let _ops = self.lock_ops().await?;
        let mut doc = Self::writable(self.view().await)?;
        let request = ProofRequest { method, arguments };
        let used = doc.check_proof(self.verifier.as_ref(), &request, proof)?;
        change(&mut doc)?;
        doc.used_proofs.insert(used);
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        self.log_entry(&entry)
            .await
            .map_err(GuardError::DoneNotShown)?;
        Ok(doc)
    }
}

#[async_trait]
impl Guard for GuardEngine {
    async fn state(&self) -> GuardState {
        match self.view().await {
            View::Doc(doc) => {
                let mut state = doc.view();
                if self.is_latched() {
                    state.mode = GuardMode::Stopped;
                }
                state
            }
            _ => GuardState::default(),
        }
    }

    async fn invite(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError> {
        let now = self.clock.now();
        let owner = self.config.owner.clone();
        let (key, signer) = (id_key(profile), id_key(&proof.signer));
        let entry = GuardLogEntry::new("invite", &signer).about(&key);
        self.signed_change(Method::Invite, &[profile.as_bytes()], proof, entry, |doc| {
            doc.check_invite(&key, &owner)?;
            doc.apply_invite(&key, &signer, now);
            Ok(())
        })
        .await?;
        Ok(())
    }

    async fn release(&self, grant: &AiGrant, proof: &GuardProof) -> Result<(), GuardError> {
        let owner = self.config.owner.clone();
        let (key, signer) = (id_key(&grant.profile), id_key(&proof.signer));
        let mut entry = GuardLogEntry::new("release", &signer).about(&key);
        entry.risk = Some(grant.risk);
        let stored = AiGrant {
            profile: key,
            risk: grant.risk,
            program: String::new(),
            goes_to: String::new(),
        };
        let arguments: [&[u8]; 2] = [grant.profile.as_bytes(), risk_name(grant.risk).as_bytes()];
        self.signed_change(Method::Release, &arguments, proof, entry, |doc| {
            doc.check_release(&stored, &signer, &owner)?;
            doc.apply_grant(stored.clone());
            Ok(())
        })
        .await?;
        Ok(())
    }

    async fn revoke_program(
        &self,
        profile: &str,
        _program: &str,
        proof: &GuardProof,
    ) -> Result<(), GuardError> {
        // Shell: the program is not used yet.
        self.revoke(profile, proof).await
    }

    async fn revoke(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError> {
        self.take_away(Method::Revoke, "revoke", profile, proof)
            .await
    }

    async fn uninvite(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError> {
        self.take_away(Method::Uninvite, "uninvite", profile, proof)
            .await
    }

    async fn stop(&self, proof: &GuardProof) -> Result<StopReport, GuardError> {
        self.pull_stop(Some(proof.clone())).await
    }

    async fn reopen(&self, proof: &GuardProof) -> Result<(), GuardError> {
        // A stop that comes in from here on is later than this reopen, and wins over it.
        let seen = self.stop_count();
        let _ops = self.lock_ops().await?;
        let mut doc = Self::writable(self.view().await)?;
        let request = ProofRequest {
            method: Method::Reopen,
            arguments: &[],
        };
        let used = doc.check_proof(self.verifier.as_ref(), &request, proof)?;
        let signer = id_key(&proof.signer);
        if !self.permission.may_reopen(&signer, &self.config.owner) {
            return Err(Refusal::NotPermitted.into());
        }
        if doc.mode == GuardMode::Open && !self.is_latched() {
            return Err(Refusal::AlreadyOpen.into());
        }
        doc.apply_reopen();
        doc.used_proofs.insert(used);
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        if self.stop_count() != seen {
            return Err(Refusal::Stopped.into());
        }
        // A marker that stays makes the next start stopped: fail-shut, and said.
        let removing = marker::remove(self.config.stop_marker.clone());
        let not_removed = tokio::time::timeout(self.config.store_wait, removing)
            .await
            .unwrap_or_else(|_| Err("the disk did not answer in time".into()))
            .err();
        if !self.clear_stop_unless_newer(seen) {
            // A stop came in while the marker went: it wrote its marker before this removed it.
            if let Some(mark) = self.unsaved_mark() {
                // Within the time limit, as every step under the lock; the retry task goes on.
                let writing = marker::write(self.config.stop_marker.clone(), mark);
                let _ = tokio::time::timeout(self.config.store_wait, writing).await;
            }
            return Err(Refusal::Stopped.into());
        }
        let not_logged = self
            .log_entry(&GuardLogEntry::new("reopen", &signer))
            .await
            .err();
        match not_removed.or(not_logged) {
            Some(why) => Err(GuardError::DoneNotShown(why)),
            None => Ok(()),
        }
    }
}
