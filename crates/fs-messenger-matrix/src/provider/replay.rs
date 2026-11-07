//! Which rooms a replay reads again after a lost answer, and how far back.
//!
//! Pattern: Specification: `worth_replaying` and `reads_to_start` are pure predicates over a room's
//! state and whether the cursor knows the room. Used by `fetch.rs` (Adapter, part of
//! `MatrixProvider`), which does the reading.

use matrix_sdk::RoomState;

/// Whether a room the SDK holds is read again after a lost answer. A joined room is. A left room is
/// only while the cursor still knows it; invited, knocked and banned rooms are not.
pub(super) fn worth_replaying(state: RoomState, known: bool) -> bool {
    match state {
        RoomState::Joined => true,
        RoomState::Left => known,
        _ => false,
    }
}

/// Whether a replayed room is read down to its start (a joined room the cursor has never read)
/// instead of down to the cursor's position.
pub(super) fn reads_to_start(state: RoomState, known: bool) -> bool {
    state == RoomState::Joined && !known
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_room_is_replayed_and_an_unknown_one_from_its_start() {
        assert!(worth_replaying(RoomState::Joined, true));
        assert!(worth_replaying(RoomState::Joined, false));
        assert!(!reads_to_start(RoomState::Joined, true));
        assert!(reads_to_start(RoomState::Joined, false));
    }

    #[test]
    fn a_left_room_is_replayed_known_or_not_but_never_from_its_start() {
        for known in [true, false] {
            assert!(worth_replaying(RoomState::Left, known), "known: {known}");
            assert!(!reads_to_start(RoomState::Left, known), "known: {known}");
        }
    }

    #[test]
    fn an_invited_or_knocked_room_is_replayed_only_if_the_cursor_knew_it() {
        for state in [RoomState::Invited, RoomState::Knocked] {
            assert!(worth_replaying(state, true), "{state:?} known");
            assert!(!worth_replaying(state, false), "{state:?} unknown");
            assert!(!reads_to_start(state, true) && !reads_to_start(state, false));
        }
    }

    /// Sync v2 never sets `Banned` (the SDK marks every room of `leave` as left); only sliding sync
    /// does. The branch stays careful all the same: a ban is never read again by the replay.
    #[test]
    fn a_banned_room_is_never_replayed() {
        for known in [true, false] {
            assert!(!worth_replaying(RoomState::Banned, known), "known: {known}");
            assert!(!reads_to_start(RoomState::Banned, known), "known: {known}");
        }
    }
}
