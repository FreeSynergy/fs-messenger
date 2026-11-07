//! What a room is to the cursor, and the move one answer makes.
//!
//! Pattern: State: `Known` is the state of a room (never accepted, member, away) and `step` and
//! `after_check` are the transition functions; one table decides every move, with and without a
//! replay. Pure: no network, no cursor.

use super::cursor::{CheckOf, Gap};

/// The state of one room as the cursor sees it at the confirmed position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Known {
    /// Never seen as accepted: an open, declined or withdrawn invitation, a knock, spam.
    Unknown,
    /// Joined at the position; everything visible up to it is delivered or in an open gap.
    Member,
    /// Accepted once, not joined at the position. `at` is the point before the leave; `None` is the
    /// start of the room.
    Away { at: Option<String> },
}

/// The part of the `/sync` answer a room is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// `join`.
    Join,
    /// `leave` (a ban shows here too).
    Leave,
    /// `invite`.
    Invite,
    /// `knock`.
    Knock,
}

/// What the answer shows of one room.
#[derive(Clone, Debug)]
pub(crate) struct Seen<'a> {
    /// The part of the answer.
    pub(crate) section: Section,
    /// An own join (sender and state key are the account) is in its state or timeline.
    pub(crate) own_join: bool,
    /// The server said the timeline is limited.
    pub(crate) limited: bool,
    /// This crate cut the timeline over its limit.
    pub(crate) cut: bool,
    /// Where `/messages` can read back from the start of the timeline.
    pub(crate) prev_batch: Option<&'a str>,
}

/// The move one answer makes for one room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    /// Deliver the timeline of the answer.
    pub(crate) deliver: bool,
    /// Read this gap through `/messages`.
    pub(crate) read: Option<Gap>,
    /// Ask this check (only own membership events).
    pub(crate) check: Option<Gap>,
    /// The room's state afterwards.
    pub(crate) becomes: Known,
}

/// The one decision table. `since` is what the answer was asked from, `next` its `next_batch`.
pub(crate) fn step(
    known: &Known,
    seen: &Seen<'_>,
    room: &str,
    since: Option<&str>,
    next: &str,
) -> Step {
    let _ = (known, seen, room, since, next);
    unimplemented!("membership::step")
}

/// A check found an own join: the gap to read now and the state afterwards.
pub(crate) fn after_check(known: &Known, check: &Gap) -> (Gap, Known) {
    let _: Option<&CheckOf> = check.check.as_ref();
    let _ = known;
    unimplemented!("membership::after_check")
}
