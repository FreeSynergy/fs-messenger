//! The engine: the state machine, its repository, and the way into the messenger.

mod admin;
mod core;
mod decide;
mod gate;
mod marker;
mod questions;
mod sensitive;
mod stop;
mod stop_conversation;
mod stop_judge;
mod store_access;
mod wiring;

pub use core::{GuardConfig, GuardEngine};
pub(crate) use store_access::View;
