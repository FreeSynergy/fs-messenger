//! The engine: the state machine, its repository, and the way into the messenger.

mod admin;
mod core;
mod decide;
mod gate;
mod stop;
mod wiring;

pub(crate) use core::View;
pub use core::{GuardConfig, GuardEngine};
