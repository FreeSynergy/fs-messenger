//! `fs-messenger-guard`: the AI guard of FreeSynergy.Messenger (ADR 0054, 0056).
//!
//! In the engine, below every frontend, without network and without a model. [`Gate`] is what anyone
//! who wants to act goes through. [`Guard`] is what a person's device uses. [`GuardEngine`] is both.
//! Tool layers get a `Gate`, never a `Guard`. See `DESIGN.md`.
//!
//! Pattern: none, allowed by section 2 (declarations and re-exports only, as coding.md section 3 asks).

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions
)]

mod api;
mod arguments;
mod clock;
mod destination;
mod device;
mod digest;
mod engine;
mod enrol;
mod error;
mod identity;
mod log;
mod model;
mod no_destinations;
mod no_marks;
mod permission;
mod program;
mod repo;
mod risk;
mod secret_name;
mod sensitivity;
mod signed;
mod state;
mod table;
mod words;

pub use api::{Gate, Guard};
pub use arguments::{Arguments, ArgumentsError};
pub use clock::{Clock, SystemClock};
pub use destination::{Destination, DestinationLookup};
pub use device::{DeviceVerifier, Ed25519Verifier};
pub use digest::arguments_digest;
pub use engine::{GuardConfig, GuardEngine};
pub use enrol::LocalEnrolment;
pub use error::{GuardError, Refusal};
pub use identity::id_key;
pub use log::GuardLogEntry;
pub use model::{
    AiGrant, Allowance, Caller, GuardMode, GuardProof, GuardState, Question, StopReport, Verdict,
};
pub use no_destinations::NoDestinations;
pub use no_marks::NoMarks;
pub use permission::{OwnerOnly, StopPermission};
pub use program::program_of;
pub use repo::{GuardRepository, STATE_ID, TursoGuardRepository};
pub use risk::can_be_released;
pub use secret_name::is_secret_name;
pub use sensitivity::{ObjectRef, Sensitivity, SensitivityLookup};
pub use signed::{CONTEXT, Method, decide_arguments, signed_bytes};
pub use table::{CapabilityRule, action_capability, capability_rule, provisional_table};
