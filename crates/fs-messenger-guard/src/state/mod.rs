//! The state machine: everything the guard knows, and every change, as pure functions.

mod admin;
mod calls;
mod decide;
mod doc;
mod proof;
mod releases;
mod stage;
mod standing;

pub(crate) use admin::{DeviceChange, StopMark, check_program_argument, usable_name};
pub(crate) use calls::{Evaluation, Judged, Limits, QuestionInput};
pub(crate) use decide::DecideInput;
pub(crate) use doc::{CallRecord, GuardDoc};
pub(crate) use proof::ProofRequest;
pub(crate) use stage::CallStage;
