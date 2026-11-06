//! The model behind the core's `Provider` trait.

mod choice;
mod facade;
mod host;
mod plan;

pub use choice::RemoteChoice;
pub use facade::ModelProvider;
pub use host::{Host, ProviderLink};
