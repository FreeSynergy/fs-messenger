//! The switch between the table and the description.
//!
//! Pattern: Adapter (table row to `CommandDescription`) with ONE switch, [`apply_support`]. Until `types.capnp` and
//! `fs-types` carry the field `supported` on a command (Thread T), the description cannot say which commands the core
//! cannot carry out: it lists ALL of them, so a frontend can show "coming later", and `Backend::run` refuses an unsupported
//! one by name. When the field exists, the body of [`apply_support`] sets it from the row, and nothing else changes: not the
//! table, not the Backend, not the frontend.

use fs_types::CommandDescription;

use super::entry::Entry;
use super::table::table;

/// Mark `command` with what the table says about it.
///
/// **The switch.** Today `CommandDescription` has no `supported` field (an invented one would not travel over the
/// wire), so this returns the command unchanged. When `fs-types` has the field: `command.supported = entry.supported;`
/// and, if it has a reason key, `command.reason = ...`. This is the only place that changes then.
#[must_use]
pub fn apply_support(command: CommandDescription, _entry: &Entry) -> CommandDescription {
    command
}

/// What the table says about command `name` of `data_type`: `Some((supported, reason))`, or `None` if there is no such command.
#[must_use]
pub fn support_of(data_type: &str, name: &str) -> Option<(bool, &'static str)> {
    table()
        .find(|e| e.data_type == data_type && e.command_name() == Some(name))
        .map(|e| (e.supported, e.reason))
}
