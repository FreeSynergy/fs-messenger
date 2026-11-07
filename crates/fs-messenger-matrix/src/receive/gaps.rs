//! Planning gaps without letting them pile up.
//!
//! Pattern: Memento, part of `MatrixCursor` (the type is spread over `cursor.rs`, `gaps.rs`, `pending.rs`, `given_up.rs` and `apply.rs`): `plan_gap(&mut self)` changes the cursor, the snapshot of where
//! the provider stands, so this is not a group of pure functions. A
//! gap of a room that touches an open one is joined to it, and a room never keeps more than
//! [`MAX_GAPS_PER_ROOM`]: the oldest two are then made one, which covers both and everything between
//! them. Nothing is dropped; a hostile server only makes the one gap read again, never the cursor grow.
//! This relies on the list holding a room's gaps oldest first, which is the order they are planned in.

use super::cursor::{Gap, MatrixCursor};

/// Checks ("did we ever join?") in all, at most; one per room. A flood of invitations is named, not kept.
pub(crate) const MAX_CHECKS: usize = 64;

/// Open gaps per room, at most. Eight is a guess; an honest server needs one or two.
pub(crate) const MAX_GAPS_PER_ROOM: usize = 8;

impl MatrixCursor {
    /// Read this later, unless that is already planned or an open gap of the room ends where this
    /// one starts (the two become one).
    pub(crate) fn plan_gap(&mut self, gap: Gap) {
        if self.gaps.contains(&gap) {
            return;
        }
        let touching = gap.to.as_ref().and_then(|to| {
            let same = |g: &&mut Gap| g.room == gap.room && g.from == *to && g.skip == 0;
            self.gaps.iter_mut().find(same)
        });
        if let Some(old) = touching {
            old.from = gap.from;
            return;
        }
        let room = gap.room.clone();
        self.gaps.push(gap);
        let of_room = |g: &Gap| g.room == room;
        while self.gaps.iter().filter(|g| of_room(g)).count() > MAX_GAPS_PER_ROOM {
            let mut at = (0..self.gaps.len()).filter(|i| of_room(&self.gaps[*i]));
            let (older, newer) = (at.next().unwrap_or(0), at.next().unwrap_or(0));
            let newer = self.gaps.remove(newer);
            let kept = &mut self.gaps[older];
            kept.from = newer.from;
            kept.skip = newer.skip;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(room: &str, from: &str, to: Option<&str>) -> Gap {
        Gap::new(room.into(), from.into(), to.map(Into::into))
    }

    #[test]
    fn a_double_is_not_planned_twice() {
        let mut c = MatrixCursor::default();
        c.plan_gap(gap("!r:x", "b", Some("a")));
        c.plan_gap(gap("!r:x", "b", Some("a")));
        assert_eq!(c.gaps.len(), 1);
    }

    #[test]
    fn a_gap_that_ends_where_an_open_one_starts_joins_it() {
        let mut c = MatrixCursor::default();
        c.plan_gap(gap("!r:x", "b", Some("a")));
        c.plan_gap(gap("!r:x", "c", Some("b")));
        assert_eq!(c.gaps, [gap("!r:x", "c", Some("a"))]);
    }

    #[test]
    fn another_room_is_not_joined() {
        let mut c = MatrixCursor::default();
        c.plan_gap(gap("!r:x", "b", Some("a")));
        c.plan_gap(gap("!s:x", "c", Some("b")));
        assert_eq!(c.gaps.len(), 2);
    }

    #[test]
    fn a_room_keeps_at_most_the_limit_and_the_whole_range_stays_covered() {
        let mut c = MatrixCursor::default();
        c.plan_gap(gap("!s:x", "z", None));
        for i in 0..50 {
            // Disjoint, newest last: a start is never the end of another.
            c.plan_gap(gap("!r:x", &format!("f{i}"), Some(&format!("t{i}"))));
        }
        let mine: Vec<_> = c.gaps.iter().filter(|g| g.room == "!r:x").collect();
        assert_eq!(mine.len(), MAX_GAPS_PER_ROOM);
        assert_eq!(c.gaps.iter().filter(|g| g.room == "!s:x").count(), 1);
        assert_eq!(mine[0].to.as_deref(), Some("t0"), "the oldest end stays");
        assert_eq!(
            mine[MAX_GAPS_PER_ROOM - 1].from,
            "f49",
            "the newest start stays"
        );
    }

    fn check(room: &str, origin: &str, to: Option<&str>) -> Gap {
        let mut g = gap(room, origin, to);
        g.check = Some(super::super::cursor::CheckOf {
            origin: origin.into(),
        });
        g
    }

    #[test]
    fn a_room_has_one_check_with_the_newest_origin_and_the_oldest_end() {
        let mut c = MatrixCursor::default();
        c.plan_gap(check("!r:x", "n1", Some("p1")));
        c.plan_gap(check("!r:x", "n2", Some("p2")));
        assert_eq!(c.gaps, [check("!r:x", "n2", Some("p1"))]);
    }

    #[test]
    fn a_check_and_a_read_never_join() {
        let mut c = MatrixCursor::default();
        c.plan_gap(gap("!r:x", "b", Some("a")));
        c.plan_gap(check("!r:x", "c", Some("b")));
        assert_eq!(c.gaps.len(), 2);
        c.plan_gap(gap("!r:x", "d", Some("c")));
        assert_eq!(c.gaps.len(), 3, "a read does not join a check either");
    }

    #[test]
    fn checks_are_bounded_and_the_overflow_is_named() {
        let mut c = MatrixCursor::default();
        for i in 0..MAX_CHECKS + 10 {
            c.plan_gap(check(&format!("!r{i}:x"), "n", Some("p")));
        }
        assert_eq!(
            c.gaps.iter().filter(|g| g.check.is_some()).count(),
            MAX_CHECKS
        );
        assert_eq!(c.given_up.len(), 10, "{:?}", c.given_up);
        assert!(
            c.given_up
                .iter()
                .all(|g| g.why == super::super::given_up::Why::Overflow)
        );
    }
}
