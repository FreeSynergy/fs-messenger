//! P1 of the review of 71541b3: the source of destinations is asked at most once for a participant and
//! a program in one operation, within one wait for the whole operation, and clearing away expired
//! questions does not ask it at all (only showing does). A hanging source must not keep the operations
//! lock for N waits: `decide` and the like stay available.

mod common;

use std::time::{Duration as Wait, Instant};

use chrono::Duration;
use common::destination::Switch;
use common::{World, args};
use fs_messenger_core::{MessageAction, Messenger as _};
use fs_messenger_guard::Verdict;

/// Six questions that have expired, one that has not, and the source answering for now.
async fn six_expired_and_one_live(
    sw: &std::sync::Arc<Switch>,
) -> (World, fs_messenger_guard::Question) {
    let w = World::working().await;
    assert!(w.engine.set_destinations(sw.clone()));
    for i in 0..6 {
        w.ask("message.delete", args(&format!("/old{i}"))).await;
    }
    w.clock.advance(Duration::minutes(9));
    let live = w.ask("message.delete", args("/live")).await;
    w.clock.advance(Duration::minutes(2)); // the six are over (10 minutes), the live one is not
    (w, live)
}

#[tokio::test]
async fn clearing_away_expired_questions_asks_the_source_nothing() {
    let sw = Switch::new();
    let (w, _live) = six_expired_and_one_live(&sw).await;
    sw.asked.lock().unwrap().clear();
    let q = w.ask("message.delete", args("/new")).await;
    assert_eq!(q.call.summary, "only this device");
    let asked = sw.asked.lock().unwrap().clone();
    assert_eq!(
        asked.len(),
        1,
        "one participant and program, one question: {asked:?}"
    );
}

#[tokio::test]
async fn a_hanging_source_holds_the_lock_for_one_wait_and_decide_stays_open() {
    let sw = Switch::new();
    let (w, live) = six_expired_and_one_live(&sw).await;
    sw.hangs_from_now();
    w.pause();
    let started = Instant::now();
    let checking = async {
        let verdict = w.check("message.delete", args("/new")).await;
        (verdict, started.elapsed())
    };
    let deciding = async {
        tokio::time::sleep(Wait::from_millis(200)).await;
        let d = w
            .owner
            .decide(&w.identity, live.message, &live.call, "allow-once");
        let result = w
            .repo
            .act(live.conversation, live.message, MessageAction::Decide(d))
            .await;
        (result, started.elapsed())
    };
    let ((verdict, checked), (decided, decided_at)) = tokio::join!(checking, deciding);
    assert!(matches!(verdict, Verdict::Ask(_)), "{verdict:?}");
    assert!(
        checked < Wait::from_secs(4),
        "one wait of 2 seconds, not one per expired question: {checked:?}"
    );
    decided.expect("decide was not locked out");
    assert!(decided_at < Wait::from_secs(8), "{decided_at:?}");
}
