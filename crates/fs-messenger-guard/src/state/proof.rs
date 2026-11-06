//! Checking a device proof against the state.
//!
//! Pattern: Specification (pure). The order matters: nothing is used up until the signature holds, so
//! nobody can burn a person's nonce with a forged proof. A signer marked as AI is refused before any
//! key is looked at (ADR 0056). Single use goes by signer and nonce, or for a decision by the signed
//! bytes, and never by the bytes of the signature, as those can differ for the same message.

use super::doc::GuardDoc;
use crate::device::DeviceVerifier;
use crate::digest::{single_use_key, unhex};
use crate::error::Refusal;
use crate::identity::id_key;
use crate::model::GuardProof;
use crate::signed::{Method, signed_bytes};

const MIN_NONCE: usize = 16;
const MAX_NONCE: usize = 64;

/// What a proof has to be about.
pub(crate) struct ProofRequest<'a> {
    pub(crate) method: Method,
    pub(crate) arguments: &'a [&'a [u8]],
}

impl GuardDoc {
    /// Whether this signer has a key that signed exactly these bytes.
    pub(crate) fn signature_holds(
        &self,
        verifier: &dyn DeviceVerifier,
        signer: &str,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), Refusal> {
        let signer = id_key(signer);
        if self.is_invited(&signer) {
            return Err(Refusal::SignerIsAi);
        }
        let keys = self.devices.get(&signer).ok_or(Refusal::BadProof)?;
        // The key type was checked on reading the state.
        let holds = keys.iter().any(|key| {
            unhex(&key.public_key)
                .is_some_and(|public| verifier.verify(&public, message, signature))
        });
        if holds {
            Ok(())
        } else {
            Err(Refusal::BadProof)
        }
    }

    /// Check a proof, and answer the key that makes it single use. Changes nothing.
    pub(crate) fn check_proof(
        &self,
        verifier: &dyn DeviceVerifier,
        request: &ProofRequest<'_>,
        proof: &GuardProof,
    ) -> Result<String, Refusal> {
        if proof.signer.is_empty() {
            return Err(Refusal::Empty("signer"));
        }
        if proof.proof.is_empty() {
            return Err(Refusal::Empty("proof"));
        }
        if proof.nonce.is_empty() {
            return Err(Refusal::Empty("nonce"));
        }
        if !(MIN_NONCE..=MAX_NONCE).contains(&proof.nonce.len()) {
            return Err(Refusal::BadProof);
        }
        let engine = unhex(&self.engine).ok_or(Refusal::Damaged)?;
        let message = signed_bytes(
            &engine,
            request.method,
            &proof.signer,
            request.arguments,
            &proof.nonce,
        )
        .ok_or(Refusal::BadProof)?;
        self.signature_holds(verifier, &proof.signer, &message, &proof.proof)?;
        let key = single_use_key(b"nonce", &[id_key(&proof.signer).as_bytes(), &proof.nonce]);
        if self.used_proofs.contains(&key) {
            return Err(Refusal::Replayed);
        }
        Ok(key)
    }
}
