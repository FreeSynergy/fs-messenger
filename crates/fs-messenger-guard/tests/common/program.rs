//! Helpers for releases per program and for marks of objects.
#![allow(dead_code)]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use fs_messenger_core::ToolCallRisk;
use fs_messenger_guard::{
    AiGrant, Arguments, Caller, Gate, GuardError, Method, ObjectRef, Sensitivity,
    SensitivityLookup, Verdict, program_of,
};

use super::{AI, PROGRAM, World};

pub fn risk_word(risk: ToolCallRisk) -> &'static str {
    match risk {
        ToolCallRisk::ReadsOnly => "readsOnly",
        ToolCallRisk::Changes => "changes",
        ToolCallRisk::Destroys => "destroys",
        ToolCallRisk::ActsForPerson => "actsForPerson",
        ToolCallRisk::ReadsSensitive => "readsSensitive",
        _ => "unknown",
    }
}

impl World {
    /// A read of an object the engine knows to be unmarked: the way a read is allowed by its class
    /// (strategy 4d(2): `check` without an object counts as marked and asks). The first source set
    /// wins, so this is for tests that set none of their own.
    pub async fn read(&self, capability: &str, args: serde_json::Value) -> Verdict {
        read_unmarked(&self.engine, capability, &args).await
    }

    /// `AI` invited, with `changes` released in the program `message` and nothing else. Calls of a
    /// class that always asks (`message.delete`, `message.send`) then ask, as they did before a
    /// release was needed for anything.
    pub async fn working() -> Self {
        let w = Self::with_ai().await;
        w.release(AI, ToolCallRisk::Changes, "changes").await;
        w
    }

    /// A release in a program.
    pub async fn release_in(
        &self,
        profile: &str,
        risk: ToolCallRisk,
        program: &str,
    ) -> Result<(), GuardError> {
        let grant = AiGrant {
            profile: profile.into(),
            risk,
            program: program.into(),
        };
        let proof = self.owner.proof(
            &self.identity,
            Method::Release,
            &[
                profile.as_bytes(),
                risk_word(risk).as_bytes(),
                program.as_bytes(),
            ],
        );
        fs_messenger_guard::Guard::release(&*self.engine, &grant, &proof).await
    }

    /// Something is released in the program of this tool, so that what always asks can ask.
    pub async fn unlock_program_of(&self, capability: &str) {
        let program = program_of(capability);
        self.release_in(AI, ToolCallRisk::ReadsOnly, &program)
            .await
            .unwrap();
    }

    /// The default release of the helpers: `message`, this device only.
    pub async fn release_default(&self, risk: ToolCallRisk) {
        self.release_in(AI, risk, PROGRAM).await.unwrap();
    }
}

/// `read` for an engine that is not in a `World`.
pub async fn read_unmarked(
    engine: &fs_messenger_guard::GuardEngine,
    capability: &str,
    args: &serde_json::Value,
) -> Verdict {
    let _ = engine.set_sensitivity(Fixed::new(Sensitivity::Unmarked));
    let arguments = Arguments::from_value(args).unwrap();
    let object = ObjectRef::new(
        &program_of(capability),
        "01900000-0000-7000-8000-0000000000aa",
    );
    let caller = Caller::new(AI);
    engine
        .check_object(&caller, capability, &arguments, &object.unwrap())
        .await
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
