//! A person's device, and a clock a test can move.
#![allow(dead_code)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
use ed25519_compact::{KeyPair, Seed};
use fs_messenger_core::{ToolCall, ToolCallDecision};
use fs_messenger_guard::{Clock, GuardProof, Method, decide_arguments, signed_bytes};
use fs_objects::ObjectId;

/// A clock a test can move.
#[derive(Default)]
pub struct TestClock(Mutex<Duration>);

impl TestClock {
    pub fn advance(&self, by: Duration) {
        let mut held = self.0.lock().unwrap();
        *held += by;
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now() + *self.0.lock().unwrap()
    }
}

/// A person's device: the private key stays in the test, as it stays on the device.
pub struct Device {
    pub pair: KeyPair,
    pub person: String,
    nonce: AtomicU64,
}

impl Device {
    pub fn new(person: &str, seed: u8) -> Self {
        Self {
            pair: KeyPair::from_seed(Seed::new([seed; 32])),
            person: person.to_string(),
            nonce: AtomicU64::new(1),
        }
    }

    pub fn public(&self) -> Vec<u8> {
        self.pair.pk.to_vec()
    }

    pub fn fresh_nonce(&self) -> Vec<u8> {
        let n = self.nonce.fetch_add(1, Ordering::SeqCst);
        let mut nonce = vec![self.pair.pk[0]; 8];
        nonce.extend_from_slice(&n.to_be_bytes());
        nonce
    }

    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        self.pair.sk.sign(bytes, None).to_vec()
    }

    /// A proof of `method` over `arguments` for this engine.
    pub fn proof(&self, engine: &[u8], method: Method, arguments: &[&[u8]]) -> GuardProof {
        let nonce = self.fresh_nonce();
        let bytes = signed_bytes(engine, method, &self.person, arguments, &nonce).unwrap();
        GuardProof {
            signer: self.person.clone(),
            nonce,
            proof: self.sign(&bytes),
        }
    }

    /// The decision a person's device sends, for the call as it was shown.
    pub fn decide(
        &self,
        engine: &[u8],
        message: ObjectId,
        call: &ToolCall,
        option: &str,
    ) -> ToolCallDecision {
        let (message, revision) = (message.to_string(), call.revision.to_be_bytes());
        let arguments = decide_arguments(
            &message,
            &call.id,
            option,
            &call.arguments_digest,
            &revision,
        );
        let bytes = signed_bytes(engine, Method::Decide, &self.person, &arguments, &[]).unwrap();
        ToolCallDecision {
            tool_call: call.id.clone(),
            option: option.to_string(),
            arguments_digest: call.arguments_digest.clone(),
            proof: self.sign(&bytes),
            signer: self.person.clone(),
            revision: call.revision,
        }
    }
}
