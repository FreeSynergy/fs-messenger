//! Tests of the consent rule (ADR 0054, 4c).
//!
//! Pattern: none, tests only (they exercise the State machine of `consent.rs`).

use super::*;

const AI: &str = "ai:claude";

fn room() -> Consent {
    Consent::open(AI, &["anna", "ben"])
}

#[test]
fn nobody_answered_the_ai_waits() {
    assert_eq!(room().outcome(), Outcome::Waiting);
    assert_eq!(room().history(), History::None);
}

#[test]
fn all_must_agree_one_agreement_is_not_enough() {
    let mut c = room();
    assert_eq!(c.answer("anna", Say::Agrees), Outcome::Waiting);
    assert_eq!(c.answer("ben", Say::Agrees), Outcome::Admitted);
}

#[test]
fn one_no_keeps_the_ai_out_whatever_the_others_say() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    assert_eq!(c.answer("ben", Say::Refuses), Outcome::Kept);
    assert_eq!(c.enter(10), Outcome::Kept);
    assert_eq!(c.history(), History::None);
}

#[test]
fn nothing_sent_before_the_entry_is_for_the_ai() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    let h = c.history();
    assert!(!h.allows(99), "a message from before");
    assert!(!h.allows(100), "the second of the entry itself is left out");
    assert!(h.allows(101));
    assert!(h.allows(500));
}

#[test]
fn a_refusal_after_the_entry_is_sticky_and_agreeing_again_does_not_bring_the_ai_back() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    assert_eq!(c.answer("ben", Say::Refuses), Outcome::MustLeave);
    assert_eq!(c.answer("ben", Say::Agrees), Outcome::MustLeave, "sticky");
    assert_eq!(c.history(), History::None);
    // Only a fresh agreement of everybody can bring it back, and then from the new entry on.
    assert_eq!(c.restart(&["anna", "ben"]), Outcome::Waiting);
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(500);
    assert!(!c.history().allows(400), "nothing of the time it was out");
    assert!(c.history().allows(501));
}

#[test]
fn what_is_sent_while_a_latecomer_has_not_answered_is_not_for_the_ai() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    assert_eq!(c.arrives("carla"), Outcome::Waiting);
    assert_eq!(
        c.history(),
        History::None,
        "it reads nothing while she is unanswered"
    );
    assert_eq!(c.answer_at("carla", Say::Agrees, 300), Outcome::Admitted);
    assert!(!c.history().allows(200), "sent in the gap");
    assert!(c.history().allows(301));
}

#[test]
fn people_who_are_present_now_are_asked_and_foreign_is_taken_over() {
    let mut c = Consent::open(AI, &["anna"]);
    c.answer("anna", Say::Agrees);
    assert_eq!(
        c.refresh(&["anna", "ben"], false),
        Outcome::Waiting,
        "ben counts"
    );
    assert_eq!(c.refresh(&["anna", "ben"], true), Outcome::WarnOnly);
}

#[test]
fn an_old_record_without_the_new_fields_still_reads() {
    let old = r#"{"ai":"x","says":{"anna":"agrees"},"enteredAt":null,"foreign":false}"#;
    let c: Consent = serde_json::from_str(old).unwrap();
    assert_eq!(c.outcome(), Outcome::Admitted);
}

#[test]
fn it_cannot_enter_before_everyone_agreed() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    assert_eq!(c.enter(5), Outcome::Waiting);
    assert_eq!(c.history(), History::None);
}

#[test]
fn withdrawing_after_the_entry_makes_the_ai_leave_and_ends_its_history() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    assert_eq!(c.withdraw("ben"), Outcome::MustLeave);
    assert_eq!(c.history(), History::None);
    assert!(!c.history().allows(200));
}

#[test]
fn withdrawing_before_the_entry_just_keeps_it_out() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    assert_eq!(c.withdraw("ben"), Outcome::Kept);
}

#[test]
fn a_newcomer_is_held_out_until_they_answer_and_the_ai_stays_meanwhile() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    c.arrives("cleo");
    assert!(c.holds_out("cleo"), "not in the conversation yet");
    assert!(!c.holds_out("anna"));
    // Waiting for her: the AI is not thrown out for a missing answer.
    assert_ne!(c.outcome(), Outcome::MustLeave);
    // She refuses: the AI must go, or she stays out; the conversation decides, the rule says leave.
    assert_eq!(c.answer("cleo", Say::Refuses), Outcome::MustLeave);
}

#[test]
fn a_newcomer_who_accepts_completes_the_agreement() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Agrees);
    c.enter(100);
    c.arrives("cleo");
    assert_eq!(c.answer("cleo", Say::Agrees), Outcome::Admitted);
    assert!(!c.holds_out("cleo"));
}

#[test]
fn someone_who_left_no_longer_counts() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    c.answer("ben", Say::Refuses);
    assert_eq!(c.leaves("ben"), Outcome::Admitted);
}

#[test]
fn an_empty_room_admits_nobody() {
    assert_eq!(Consent::open(AI, &[]).outcome(), Outcome::Waiting);
}

#[test]
fn a_foreign_program_is_warned_about_never_held_out() {
    let mut c = Consent::foreign(AI, &["anna"]);
    c.answer("anna", Say::Agrees);
    assert_eq!(c.outcome(), Outcome::WarnOnly);
    assert_eq!(c.history(), History::None, "we promise nothing there");
}

#[test]
fn an_answer_from_a_stranger_changes_nothing() {
    let mut c = room();
    c.answer("mallory", Say::Agrees);
    c.answer("anna", Say::Agrees);
    assert_eq!(c.outcome(), Outcome::Waiting);
}

#[test]
fn it_survives_a_round_trip_through_json() {
    let mut c = room();
    c.answer("anna", Say::Agrees);
    let back: Consent = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}
