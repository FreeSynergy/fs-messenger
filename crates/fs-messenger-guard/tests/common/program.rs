//! Helpers for releases per program and for marks of objects.
#![allow(dead_code)]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use fs_messenger_core::ToolCallRisk;
use fs_messenger_guard::{
    AiGrant, GuardError, Method, ObjectRef, Sensitivity, SensitivityLookup, program_of,
};

use super::{AI, GOES_TO, PROGRAM, World};

pub fn risk_word(risk: ToolCallRisk) -> &'static str {
    match risk {
        ToolCallRisk::ReadsOnly => "readsOnly",
        ToolCallRisk::Changes => "changes",
        ToolCallRisk::Destroys => "destroys",
        ToolCallRisk::ActsForPerson => "actsForPerson",
        _ => "unknown",
    }
}

impl World {
    /// `AI` invited, with `changes` released in the program `message` and nothing else. Calls of a
    /// class that always asks (`message.delete`, `message.send`) then ask, as they did before a
    /// release was needed for anything.
    pub async fn working() -> Self {
        let w = Self::with_ai().await;
        w.release(AI, ToolCallRisk::Changes, "changes").await;
        w
    }

    /// A release in a program, with the words the person gave for where the text goes.
    pub async fn release_in(
        &self,
        profile: &str,
        risk: ToolCallRisk,
        program: &str,
        goes_to: &str,
    ) -> Result<(), GuardError> {
        let grant = AiGrant {
            profile: profile.into(),
            risk,
            program: program.into(),
            goes_to: goes_to.into(),
        };
        let proof = self.owner.proof(
            &self.identity,
            Method::Release,
            &[
                profile.as_bytes(),
                risk_word(risk).as_bytes(),
                program.as_bytes(),
                goes_to.as_bytes(),
            ],
        );
        fs_messenger_guard::Guard::release(&*self.engine, &grant, &proof).await
    }

    /// The default release of the helpers: `message`, this device only.
    pub async fn release_default(&self, risk: ToolCallRisk) {
        self.release_in(AI, risk, PROGRAM, GOES_TO).await.unwrap();
    }
}

/// A source of marks that answers one thing, and counts its questions.
pub struct Fixed {
    pub answer: Sensitivity,
    pub asked: AtomicUsize,
}

impl Fixed {
    pub fn new(answer: Sensitivity) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl SensitivityLookup for Fixed {
    async fn sensitivity(&self, _object: &ObjectRef) -> Sensitivity {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.answer
    }
}

/// A source that never answers.
pub struct Hangs;

#[async_trait]
impl SensitivityLookup for Hangs {
    async fn sensitivity(&self, _object: &ObjectRef) -> Sensitivity {
        std::future::pending().await
    }
}

/// A source that panics.
pub struct Panics;

#[async_trait]
impl SensitivityLookup for Panics {
    async fn sensitivity(&self, _object: &ObjectRef) -> Sensitivity {
        panic!("the source broke");
    }
}

pub fn object() -> ObjectRef {
    ObjectRef::new("message", "01900000-0000-7000-8000-0000000000aa").unwrap()
}

pub fn program(capability: &str) -> String {
    program_of(capability)
}
