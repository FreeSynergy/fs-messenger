//! Whether an object that is read is marked as sensitive (ADR 0054, "Lesen ist nicht harmlos").
//!
//! Pattern: Strategy: trait [`SensitivityLookup`]. The engine asks it for the object a read names, and
//! the source can change: later it is `envelope.sensitive` of `fs-store` (ADR 0029), set by the program
//! the object belongs to or by the person, never by an AI or a provider. [`ObjectRef`] is a Value
//! object: small, compared by value, valid on construction.
//!
//! `Unknown` counts as `Marked`, and so does an answer that fails, takes too long or panics: in doubt
//! the read asks. Nothing in a call's arguments is ever read as a mark, in either direction.

use async_trait::async_trait;

use crate::error::Refusal;
use crate::state::usable_name;

/// What a source says about one object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensitivity {
    /// Marked by the program of the object or by the person.
    Marked,
    /// Known, and not marked.
    Unmarked,
    /// The source cannot say. Counts as marked.
    Unknown,
}

impl Sensitivity {
    /// Whether the read has to ask. Only `Unmarked` is free of it.
    pub fn asks(self) -> bool {
        self != Self::Unmarked
    }
}

/// The object a read names: the program it belongs to, and its id there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    program: String,
    id: String,
}

impl ObjectRef {
    /// Both parts need to be usable names after trimming (not empty, nothing that hides text): a part
    /// of white space alone is no name.
    pub fn new(program: &str, id: &str) -> Result<Self, Refusal> {
        let (program, id) = (program.trim().to_lowercase(), id.trim().to_string());
        if !usable_name(&program) || !usable_name(&id) {
            return Err(Refusal::BadName);
        }
        Ok(Self { program, id })
    }

    /// The program the object belongs to.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Its id in that program.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Where the engine learns whether an object is marked.
#[async_trait]
pub trait SensitivityLookup: Send + Sync {
    /// The mark of this object. A source that cannot tell answers `Unknown`.
    async fn sensitivity(&self, object: &ObjectRef) -> Sensitivity;
}
