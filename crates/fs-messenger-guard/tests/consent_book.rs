//! The agreements of ADR 0054 in a store: the notice before entry, everybody agrees, later arrivals,
//! withdrawing, no history from before the entry, a foreign room only warns, and it all survives a restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};
use fs_messenger_guard::{Clock, ConsentBook, ConsentOutcome as Outcome, History, Say};
use fs_objects::{ObjectId, Store, TursoStore};

const AI: &str = "https://example.org/people/claude";

struct Manual(AtomicI64);

impl Clock for Manual {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).unwrap()
    }
}

async fn book(store: Arc<TursoStore>, at: i64) -> (ConsentBook, Arc<Manual>) {
    let clock = Arc::new(Manual(AtomicI64::new(at)));
    let store: Arc<dyn Store> = store;
    (
        ConsentBook::new(store, clock.clone(), "https://example.org/users/engine"),
        clock,
    )
}

async fn store() -> Arc<TursoStore> {
    Arc::new(TursoStore::open_in_memory().await.unwrap())
}

#[tokio::test]
async fn nobody_is_in_before_everybody_agreed_and_the_entry_starts_the_history() {
    let (b, clock) = book(store().await, 1_000).await;
    let room = ObjectId::new();
    assert_eq!(
        b.outcome(room, AI).await.unwrap(),
        None,
        "no notice, no agreement"
    );
    assert_eq!(b.history(room, AI).await.unwrap(), History::None);

    assert_eq!(
        b.open(room, AI, &["anna", "ben"], false).await.unwrap(),
        Outcome::Waiting
    );
    b.answer(room, AI, "anna", Say::Agrees).await.unwrap();
    assert_eq!(
        b.enter(room, AI).await.unwrap(),
        Outcome::Waiting,
        "ben has not answered"
    );
    assert!(!b.may_read(room, AI, 2_000).await.unwrap());

    b.answer(room, AI, "ben", Say::Agrees).await.unwrap();
    clock.0.store(1_500, Ordering::SeqCst);
    assert_eq!(b.enter(room, AI).await.unwrap(), Outcome::Admitted);
    assert!(
        !b.may_read(room, AI, 1_500).await.unwrap(),
        "nothing from before the entry, nor from its own second"
    );
    assert!(b.may_read(room, AI, 1_501).await.unwrap());
}

#[tokio::test]
async fn one_no_keeps_the_ai_out_and_a_later_person_holds_out_until_they_answer() {
    let (b, _) = book(store().await, 10).await;
    let (room, other) = (ObjectId::new(), ObjectId::new());
    b.open(room, AI, &["anna", "ben"], false).await.unwrap();
    b.answer(room, AI, "anna", Say::Agrees).await.unwrap();
    assert_eq!(
        b.answer(room, AI, "ben", Say::Refuses).await.unwrap(),
        Outcome::Kept
    );
    assert_eq!(b.enter(room, AI).await.unwrap(), Outcome::Kept);
    assert!(!b.may_read(room, AI, 99).await.unwrap());
    // Another room is another agreement.
    assert_eq!(b.outcome(other, AI).await.unwrap(), None);

    b.open(other, AI, &["anna"], false).await.unwrap();
    b.answer(other, AI, "anna", Say::Agrees).await.unwrap();
    b.enter(other, AI).await.unwrap();
    b.arrives(other, AI, "carla").await.unwrap();
    assert!(b.holds_out(other, AI, "carla").await.unwrap());
    b.answer(other, AI, "carla", Say::Agrees).await.unwrap();
    assert!(!b.holds_out(other, AI, "carla").await.unwrap());
}

#[tokio::test]
async fn withdrawing_after_the_entry_sends_the_ai_out_and_closes_the_history() {
    let (b, _) = book(store().await, 10).await;
    let room = ObjectId::new();
    b.open(room, AI, &["anna"], false).await.unwrap();
    b.answer(room, AI, "anna", Say::Agrees).await.unwrap();
    b.enter(room, AI).await.unwrap();
    assert!(b.may_read(room, AI, 12).await.unwrap());
    assert_eq!(
        b.withdraw(room, AI, "anna").await.unwrap(),
        Outcome::MustLeave
    );
    assert!(
        !b.may_read(room, AI, 11).await.unwrap(),
        "it must go: nothing more to read"
    );
}

#[tokio::test]
async fn a_foreign_room_only_warns_and_the_answers_survive_a_restart() {
    let shared = store().await;
    let (b, _) = book(shared.clone(), 10).await;
    let (room, element) = (ObjectId::new(), ObjectId::new());
    assert_eq!(
        b.open(element, AI, &["anna"], true).await.unwrap(),
        Outcome::WarnOnly
    );
    assert!(
        !b.may_read(element, AI, 11).await.unwrap(),
        "we never hand history to a foreign AI"
    );

    b.open(room, AI, &["anna", "ben"], false).await.unwrap();
    b.answer(room, AI, "anna", Say::Agrees).await.unwrap();
    drop(b);
    // A second `open` for the same pair keeps what was answered.
    let (again, _) = book(shared, 20).await;
    assert_eq!(
        again.open(room, AI, &["anna", "ben"], false).await.unwrap(),
        Outcome::Waiting
    );
    again.answer(room, AI, "ben", Say::Agrees).await.unwrap();
    assert_eq!(again.enter(room, AI).await.unwrap(), Outcome::Admitted);
}

#[test]
fn the_notice_names_the_ai_and_where_the_text_goes() {
    let text = ConsentBook::notice("Claude", Some("Anthropic"), false);
    assert!(
        text.contains("Claude") && text.contains("Anthropic"),
        "{text}"
    );
    assert!(ConsentBook::notice("Claude", None, false).contains("unbekannt"));
    let warning = ConsentBook::notice("Claude", None, true);
    assert!(
        warning.contains("verlassen") && !warning.contains("{ai}"),
        "{warning}"
    );
}
