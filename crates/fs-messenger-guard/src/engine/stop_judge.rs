//! Who may pull the emergency stop: the judging of its proof.
//!
//! Pattern: Facade (a step of `GuardEngine`'s stop, split from `stop.rs`). Verifier and permission are
//! foreign Strategy objects; they run on a thread of their own and within `judge_wait`, so one that
//! panics or never answers makes the stop one by nobody instead of losing it.

use std::sync::Arc;

use tokio::time::timeout;

use super::core::GuardEngine;
use super::marker;
use super::store_access::View;
use crate::error::Refusal;
use crate::identity::id_key;
use crate::model::GuardProof;
use crate::signed::Method;
use crate::state::{ProofRequest, StopMark};

/// Who a stop from the local console is by.
pub(crate) const LOCAL_CONSOLE: &str = "urn:freesynergy:messenger:local-console";

impl GuardEngine {
    /// Who may pull it. Answers the mark, or why not. With no readable state there is no way to ask.
    async fn judge(&self, proof: Option<&GuardProof>) -> Result<StopMark, Refusal> {
        let at = self.clock.now();
        let Some(proof) = proof else {
            return Ok(StopMark {
                by: LOCAL_CONSOLE.to_string(),
                at,
                used: None,
            });
        };
        let View::Doc(doc) = self.view().await else {
            return Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            });
        };
        // Verifier and permission are plain calls that may block: a thread of their own, so that
        // the time limit of `judge_surely` can fire whatever they do.
        let (verifier, permission) = (self.verifier.clone(), self.permission.clone());
        let (owner, proof) = (self.config.owner.clone(), proof.clone());
        let judged = tokio::task::spawn_blocking(move || {
            let request = ProofRequest {
                method: Method::Stop,
                arguments: &[],
            };
            let used = doc.check_proof(verifier.as_ref(), &request, &proof)?;
            let signer = id_key(&proof.signer);
            if !permission.may_stop(&signer, &owner) {
                return Err(Refusal::NotPermitted);
            }
            Ok(StopMark {
                by: signer,
                at,
                used: Some(used),
            })
        });
        // A panic there is a stop by nobody, like no answer in time.
        judged.await.unwrap_or_else(|_| {
            Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            })
        })
    }

    /// `judge` in a task of its own, within `judge_wait`: a foreign verifier or permission that
    /// panics or never answers must not end this stop, nor hold it up, before its marker is written.
    /// What cannot be judged in time is a stop by nobody: fail-shut, as when the state cannot be read.
    pub(super) async fn judge_surely(
        self: &Arc<Self>,
        proof: Option<GuardProof>,
    ) -> Result<StopMark, Refusal> {
        let judge = self.clone();
        let at = self.clock.now();
        let task = tokio::spawn(async move { judge.judge(proof.as_ref()).await });
        match timeout(self.config.judge_wait, task).await {
            Ok(Ok(judged)) => judged,
            // Panicked, or no answer in time.
            _ => Ok(StopMark {
                by: marker::NOBODY.to_string(),
                at,
                used: None,
            }),
        }
    }
}
