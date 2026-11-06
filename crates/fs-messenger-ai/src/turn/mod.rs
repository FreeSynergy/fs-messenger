//! One turn of a model: from a person's message to a stored answer.

mod assemble;
mod failure;
mod finish;
mod history;
mod live;
mod registry;
mod run;
mod shared;
mod tools;

pub use failure::TurnFailure;
pub(crate) use history::History;
pub(crate) use live::LiveFeed;
pub use live::LiveTurn;
pub(crate) use registry::{CONVERSATION_SCHEME, Registry};
pub(crate) use run::{TurnInput, start};
pub(crate) use shared::Shared;
