//! The state machine: everything the guard knows, and every change, as pure functions.

mod admin;
mod calls;
mod decide;
mod doc;
mod proof;
mod stage;

pub(crate) use admin::{DeviceChange, StopMark, usable_name};
pub(crate) use calls::{Evaluation, Limits, QuestionInput};
pub(crate) use decide::DecideInput;
pub(crate) use doc::{CallRecord, GuardDoc};
pub(crate) use proof::ProofRequest;
pub(crate) use stage::CallStage;
