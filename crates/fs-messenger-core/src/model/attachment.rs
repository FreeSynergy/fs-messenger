//! [`Attachment`] and [`Reaction`].
//!
//! Pattern: Value objects. The contract's `blob` capability is, in the core, the id of a store
//! object that holds the bytes as its immutable original (provisional, see `DESIGN.md`).

use fs_objects::ObjectId;
use serde::{Deserialize, Serialize};

/// A file on a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// File name.
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// Size in bytes.
    pub size: u64,
    /// The store object with the bytes; read them with `Messenger::attachment`.
    pub blob: ObjectId,
}

/// A reaction on a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    /// The reaction key, for example an emoji.
    pub key: String,
    /// How many gave it.
    pub count: u32,
    /// Whether I am one of them.
    pub mine: bool,
}
