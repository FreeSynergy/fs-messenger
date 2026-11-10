//! The type description of the messenger (ADR 0066): what a frontend can show and do, made from ONE table.
//!
//! Pattern: Facade — declares the parts. `table.rs` is the capability table (what the core can do and why not),
//! `types.rs` the `DescribesType` implementations that read it, `support.rs` the one switch for the field `supported`,
//! `entry.rs` the shapes. See `DESIGN.md`, "Type description".

mod entry;
mod support;
mod table;
mod types;

pub use entry::{Delivery, Entry, In};
pub use support::{apply_support, support_of};
pub use table::table;
pub use types::{
    DraftRecord, PROGRAM, command_label, commands_of, field_label, registry, type_label,
};
