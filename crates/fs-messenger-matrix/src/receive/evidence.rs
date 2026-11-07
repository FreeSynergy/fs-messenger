//! Whether the account joined a room itself.
//!
//! Pattern: Specification: `is_own_join` is a pure predicate over an event's JSON; `any_own_join`
//! combines it by "any" over several events. Combining different predicates is not used.

/// Whether this event is the account's own join: `m.room.member`, `membership` `join`, and both
/// `sender` and `state_key` are the account. A profile change (join to join) counts; an invitation
/// or a join someone else sent does not.
pub(crate) fn is_own_join(own: &str, json: &str) -> bool {
    let _ = (own, json);
    unimplemented!("evidence::is_own_join")
}

/// Whether any of these events is the account's own join.
pub(crate) fn any_own_join<'a>(own: &str, jsons: impl IntoIterator<Item = &'a str>) -> bool {
    let _ = (own, jsons);
    unimplemented!("evidence::any_own_join")
}
