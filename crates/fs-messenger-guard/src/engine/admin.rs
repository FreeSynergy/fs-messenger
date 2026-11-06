//! [`Guard`] on the engine, apart from the stop: invite, release, revoke, reopen.
//!
//! Pattern: Template method: `signed_change` fixes the order (lock, read, proof, rule, copy, write, log).
//! A refusal uses up nothing and writes nothing. Reopen writes before it opens: if it cannot be
//! written, it stays shut.

use async_trait::async_trait;

use super::core::{GuardEngine, View};
use crate::api::Guard;
use crate::error::{GuardError, Refusal};
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
        let _ops = self.ops.lock().await;
        let mut doc = Self::writable(self.view().await)?;
        let request = ProofRequest { method, arguments };
        let used = doc.check_proof(self.verifier.as_ref(), &request, proof)?;
        change(&mut doc)?;
        doc.used_proofs.insert(used);
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        self.repo
            .append(&entry)
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
        let entry = GuardLogEntry::new("invite", &proof.signer).about(profile);
        self.signed_change(Method::Invite, &[profile.as_bytes()], proof, entry, |doc| {
            doc.check_invite(profile, &owner)?;
            doc.apply_invite(profile, &proof.signer, now);
            Ok(())
        })
        .await?;
        Ok(())
    }

    async fn release(&self, grant: &AiGrant, proof: &GuardProof) -> Result<(), GuardError> {
        let owner = self.config.owner.clone();
        let mut entry = GuardLogEntry::new("release", &proof.signer).about(&grant.profile);
        entry.risk = Some(grant.risk);
        let arguments: [&[u8]; 2] = [grant.profile.as_bytes(), risk_name(grant.risk).as_bytes()];
        self.signed_change(Method::Release, &arguments, proof, entry, |doc| {
            doc.check_release(grant, &proof.signer, &owner)?;
            doc.apply_grant(grant.clone());
            Ok(())
        })
        .await?;
        Ok(())
    }

    async fn revoke(&self, profile: &str, proof: &GuardProof) -> Result<(), GuardError> {
        let owner = self.config.owner.clone();
        let entry = GuardLogEntry::new("revoke", &proof.signer).about(profile);
        let mut cancelled = vec![];
        let doc = self
            .signed_change(Method::Revoke, &[profile.as_bytes()], proof, entry, |doc| {
                doc.check_revoke(profile, &proof.signer, &owner)?;
                cancelled = doc.apply_revoke(profile);
                Ok(())
            })
            .await?;
        let mut problems = vec![];
        for id in &cancelled {
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

    async fn stop(&self, proof: &GuardProof) -> Result<StopReport, GuardError> {
        self.pull_stop(proof).await
    }

    async fn reopen(&self, proof: &GuardProof) -> Result<(), GuardError> {
        let _ops = self.ops.lock().await;
        let mut doc = Self::writable(self.view().await)?;
        let request = ProofRequest {
            method: Method::Reopen,
            arguments: &[],
        };
        let used = doc.check_proof(self.verifier.as_ref(), &request, proof)?;
        if !self
            .permission
            .may_reopen(&proof.signer, &self.config.owner)
        {
            return Err(Refusal::NotPermitted.into());
        }
        if doc.mode == GuardMode::Open && !self.is_latched() {
            return Err(Refusal::AlreadyOpen.into());
        }
        doc.apply_reopen();
        doc.used_proofs.insert(used);
        self.save(&doc).await.map_err(GuardError::NotPersisted)?;
        self.clear_stop();
        let entry = GuardLogEntry::new("reopen", &proof.signer);
        self.repo
            .append(&entry)
            .await
            .map_err(GuardError::DoneNotShown)
    }
}
