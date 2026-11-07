//! Where the state lives: the trait, and the store behind it.

mod traits;
mod turso;

pub use traits::GuardRepository;
pub use turso::{STATE_ID, TursoGuardRepository};
