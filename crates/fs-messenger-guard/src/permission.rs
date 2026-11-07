//! Who may pull and open the emergency stop.
//!
//! Pattern: Strategy. ADR 0054 point 5: the stop is a normal permission setting, also in groups: if
//! someone gives everyone the right, everyone may. The first implementation knows one person. Groups
//! come later and answer `Unsupported`. An AI never gets here: the engine refuses a signer marked as AI
//! before asking this (ADR 0056), so no implementation can let one through.

use fs_interfaces::provider::Unsupported;

/// Who may pull the stop.
pub trait StopPermission: Send + Sync {
    /// May this person pull it for the whole program?
    fn may_stop(&self, person: &str, owner: &str) -> bool;
    /// May this person open it again?
    fn may_reopen(&self, person: &str, owner: &str) -> bool;
    /// May this person pull it for a group? Groups come later.
    fn may_stop_group(&self, _group: &str, _person: &str) -> Result<bool, Unsupported> {
        Err(Unsupported::new(
            "guard.stop.group",
            "a stop per group is a permission setting (ADR 0054), and groups are not built here yet",
        ))
    }
}

/// Only the person who owns the program.
#[derive(Clone, Copy, Debug, Default)]
pub struct OwnerOnly;

impl StopPermission for OwnerOnly {
    fn may_stop(&self, person: &str, owner: &str) -> bool {
        !owner.is_empty() && person == owner
    }
    fn may_reopen(&self, person: &str, owner: &str) -> bool {
        !owner.is_empty() && person == owner
    }
}
