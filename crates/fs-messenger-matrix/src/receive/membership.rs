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

/// A read of the timeline's past: from where `/messages` can start (the whole answer was cut: its
/// end; else the start of the timeline) down to `to`. `always` reads even if the server did not
/// say `limited`. `None` if there is nowhere to start from.
fn fill(seen: &Seen<'_>, room: &str, next: &str, to: Option<&str>, always: bool) -> Option<Gap> {
    let from = if seen.cut {
        next
    } else if always || seen.limited {
        seen.prev_batch?
    } else {
        return None;
    };
    Some(Gap::new(room.into(), from.into(), to.map(Into::into)))
}

/// The question "did we ever join in (`since`, `next`]?".
fn ask(room: &str, since: Option<&str>, next: &str) -> Gap {
    let mut gap = Gap::new(room.into(), next.into(), since.map(Into::into));
    gap.check = Some(CheckOf {
        origin: next.into(),
    });
    gap
}

/// The one decision table. `since` is what the answer was asked from, `next` its `next_batch`.
pub(crate) fn step(
    known: &Known,
    seen: &Seen<'_>,
    room: &str,
    since: Option<&str>,
    next: &str,
) -> Step {
    let at = |p: Option<&str>| Known::Away {
        at: p.map(Into::into),
    };
    let moves = |deliver, read, check, becomes| Step {
        deliver,
        read,
        check,
        becomes,
    };
    match (known, seen.section) {
        (Known::Unknown, Section::Join) => {
            let read = fill(seen, room, next, None, true);
            moves(true, read, None, Known::Member)
        }
        (Known::Unknown, Section::Leave) if seen.own_join => {
            let read = fill(seen, room, next, None, true);
            moves(true, read, None, at(since))
        }
        (Known::Unknown, _) => moves(false, None, Some(ask(room, since, next)), Known::Unknown),
        (Known::Member, Section::Join) => {
            let read = fill(seen, room, next, since, false);
            moves(true, read, None, Known::Member)
        }
        (Known::Member, Section::Leave) => {
            let read = fill(seen, room, next, since, false);
            moves(true, read, None, at(since))
        }
        (Known::Member, Section::Invite | Section::Knock) => {
            let read = Gap::new(room.into(), next.into(), since.map(Into::into));
            moves(false, Some(read), None, at(since))
        }
        (Known::Away { at: back }, Section::Join) => {
            let read = fill(seen, room, next, back.as_deref(), true);
            moves(true, read, None, Known::Member)
        }
        (Known::Away { at: back }, Section::Leave) if seen.own_join => {
            let read = fill(seen, room, next, back.as_deref(), true);
            moves(true, read, None, known.clone())
        }
        (Known::Away { .. }, _) => moves(false, None, Some(ask(room, since, next)), known.clone()),
    }
}

/// A check found an own join: the gap to read now and the state afterwards. The read starts where
/// the check began (`origin`) and ends at the start of the room if the room was never known, at the
/// point of the earlier leave if it was.
pub(crate) fn after_check(known: &Known, check: &Gap) -> (Gap, Known) {
    let origin = check.check.as_ref().map_or(&check.from, |c| &c.origin);
    let (to, becomes) = match known {
        Known::Unknown => (
            None,
            Known::Away {
                at: check.to.clone(),
            },
        ),
        Known::Away { at } => (at.clone(), known.clone()),
        Known::Member => (check.to.clone(), Known::Member),
    };
    (Gap::new(check.room.clone(), origin.clone(), to), becomes)
}

#[cfg(test)]
#[path = "membership_tests.rs"]
mod tests;
