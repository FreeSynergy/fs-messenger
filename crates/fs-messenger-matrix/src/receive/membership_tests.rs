//! Pattern: none, a test file: it tests the State of `membership.rs` and has no pattern of its own.
//!
//! One test per cell of the decision table (DESIGN.md, "Room states"): state of the room at the
//! position, part of the answer, own join seen or not, limited, cut.

#![allow(clippy::unnecessary_wraps)] // helpers that build the `Option`s of a `Step`

use super::*;

const ROOM: &str = "!r:x";

fn seen(section: Section, own_join: bool, limited: bool, cut: bool) -> Seen<'static> {
    Seen {
        section,
        own_join,
        limited,
        cut,
        prev_batch: Some("pb"),
    }
}

fn read(from: &str, to: Option<&str>) -> Option<Gap> {
    Some(Gap::new(ROOM.into(), from.into(), to.map(Into::into)))
}

fn check() -> Option<Gap> {
    let mut gap = Gap::new(ROOM.into(), "n".into(), Some("p".into()));
    gap.check = Some(CheckOf { origin: "n".into() });
    Some(gap)
}

fn away(at: Option<&str>) -> Known {
    Known::Away {
        at: at.map(Into::into),
    }
}

fn run(known: &Known, seen: &Seen<'_>) -> Step {
    step(known, seen, ROOM, Some("p"), "n")
}

fn want(deliver: bool, read: Option<Gap>, check: Option<Gap>, becomes: Known) -> Step {
    Step {
        deliver,
        read,
        check,
        becomes,
    }
}

#[test]
fn unknown_joined_reads_the_history_once_whatever_the_limit() {
    let known = Known::Unknown;
    for limited in [false, true] {
        let got = run(&known, &seen(Section::Join, false, limited, false));
        assert_eq!(got, want(true, read("pb", None), None, Known::Member));
    }
    let cut = run(&known, &seen(Section::Join, false, true, true));
    assert_eq!(cut, want(true, read("n", None), None, Known::Member));
    let mut no_prev = seen(Section::Join, false, false, false);
    no_prev.prev_batch = None;
    let none = run(&known, &no_prev);
    assert_eq!(none, want(true, None, None, Known::Member));
}

#[test]
fn unknown_left_with_an_own_join_is_read_from_the_start_and_is_away() {
    let got = run(&Known::Unknown, &seen(Section::Leave, true, false, false));
    assert_eq!(got, want(true, read("pb", None), None, away(Some("p"))));
}

#[test]
fn unknown_left_without_an_own_join_delivers_nothing_and_asks() {
    for limited in [false, true] {
        let got = run(
            &Known::Unknown,
            &seen(Section::Leave, false, limited, false),
        );
        assert_eq!(got, want(false, None, check(), Known::Unknown), "{limited}");
    }
}

#[test]
fn unknown_invited_or_knocked_asks_and_stays_unknown() {
    for section in [Section::Invite, Section::Knock] {
        let got = run(&Known::Unknown, &seen(section, false, false, false));
        assert_eq!(got, want(false, None, check(), Known::Unknown));
    }
}

#[test]
fn a_member_joined_reads_down_to_the_position_only_when_limited_or_cut() {
    let m = Known::Member;
    let plain = run(&m, &seen(Section::Join, false, false, false));
    assert_eq!(plain, want(true, None, None, Known::Member));
    let limited = run(&m, &seen(Section::Join, false, true, false));
    assert_eq!(
        limited,
        want(true, read("pb", Some("p")), None, Known::Member)
    );
    let cut = run(&m, &seen(Section::Join, false, true, true));
    assert_eq!(cut, want(true, read("n", Some("p")), None, Known::Member));
}

#[test]
fn a_member_who_left_is_away_at_the_position() {
    let m = Known::Member;
    let plain = run(&m, &seen(Section::Leave, false, false, false));
    assert_eq!(plain, want(true, None, None, away(Some("p"))));
    let limited = run(&m, &seen(Section::Leave, false, true, false));
    assert_eq!(
        limited,
        want(true, read("pb", Some("p")), None, away(Some("p")))
    );
    let with_join = run(&m, &seen(Section::Leave, true, false, false));
    assert_eq!(with_join, want(true, None, None, away(Some("p"))));
}

#[test]
fn a_member_invited_or_knocked_again_is_read_down_to_the_position_and_is_away() {
    for section in [Section::Invite, Section::Knock] {
        let got = run(&Known::Member, &seen(section, false, false, false));
        assert_eq!(
            got,
            want(false, read("n", Some("p")), None, away(Some("p")))
        );
    }
}

#[test]
fn away_and_joined_again_reads_the_absence_always() {
    let known = away(Some("a"));
    let plain = run(&known, &seen(Section::Join, false, false, false));
    assert_eq!(
        plain,
        want(true, read("pb", Some("a")), None, Known::Member)
    );
    let cut = run(&known, &seen(Section::Join, false, true, true));
    assert_eq!(cut, want(true, read("n", Some("a")), None, Known::Member));
    let from_start = run(&away(None), &seen(Section::Join, false, false, false));
    assert_eq!(
        from_start,
        want(true, read("pb", None), None, Known::Member)
    );
}

#[test]
fn away_and_left_again_with_an_own_join_reads_the_absence_and_stays_away() {
    let known = away(Some("a"));
    let got = run(&known, &seen(Section::Leave, true, false, false));
    assert_eq!(
        got,
        want(true, read("pb", Some("a")), None, away(Some("a")))
    );
}

#[test]
fn away_and_left_again_without_an_own_join_reads_the_absence_and_stays_away() {
    // The account agreed to this room once: no check (it cannot see a return the server hides).
    let got = run(&away(Some("a")), &seen(Section::Leave, false, true, false));
    assert_eq!(
        got,
        want(false, read("n", Some("a")), None, away(Some("a")))
    );
    let from_start = run(&away(None), &seen(Section::Leave, false, false, false));
    assert_eq!(from_start, want(false, read("n", None), None, away(None)));
}

#[test]
fn away_and_invited_or_knocked_reads_the_absence_and_stays_away() {
    for section in [Section::Invite, Section::Knock] {
        let got = run(&away(Some("a")), &seen(section, false, false, false));
        assert_eq!(
            got,
            want(false, read("n", Some("a")), None, away(Some("a")))
        );
    }
}

#[test]
fn a_check_that_finds_a_join_reads_on_from_where_it_began() {
    let mut gap = Gap::new(ROOM.into(), "mid".into(), Some("p".into()));
    gap.check = Some(CheckOf { origin: "n".into() });
    let (read_gap, becomes) = after_check(&Known::Unknown, &gap);
    assert_eq!(read_gap, Gap::new(ROOM.into(), "n".into(), None));
    assert_eq!(becomes, away(Some("p")));
    let (read_gap, becomes) = after_check(&away(Some("a")), &gap);
    assert_eq!(
        read_gap,
        Gap::new(ROOM.into(), "n".into(), Some("a".into()))
    );
    assert_eq!(becomes, away(Some("a")));
    let (read_gap, becomes) = after_check(&Known::Member, &gap);
    assert_eq!(
        read_gap,
        Gap::new(ROOM.into(), "n".into(), Some("p".into()))
    );
    assert_eq!(becomes, Known::Member);
}
