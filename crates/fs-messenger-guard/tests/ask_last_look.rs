//! P3 of the review of 71541b3: right before every `Ask` that goes out, the latch is looked at once more
//! (as before `Allow`). The destination is looked up first and that can take up to a wait.

mod common;

use std::time::Duration as Wait;

use common::destination::Switch;
use common::{AI, World, args};
use fs_messenger_guard::{Arguments, Caller, Gate, Guard, Refusal, Verdict};

#[tokio::test]
async fn a_stop_during_the_lookup_is_not_answered_with_a_question_again() {
    let w = World::working().await;
    let sw = Switch::new();
    assert!(w.engine.set_destinations(sw.clone()));
    w.ask("message.delete", args("/m")).await;
    sw.hangs_from_now();
    w.pause();
    let stopping = async {
        tokio::time::sleep(Wait::from_millis(300)).await;
        w.engine.stop(&w.stop_proof()).await
    };
    let (verdict, stopped) = tokio::join!(w.check("message.delete", args("/m")), stopping);
    stopped.unwrap();
    assert_eq!(verdict, Verdict::Refuse(Refusal::Stopped));
}

#[tokio::test]
async fn a_stop_during_the_lookup_of_a_revise_is_not_answered_with_a_question() {
    let w = World::working().await;
    let sw = Switch::new();
    assert!(w.engine.set_destinations(sw.clone()));
    let q = w.ask("message.delete", args("/m")).await;
    sw.hangs_from_now();
    w.pause();
    let again = Arguments::from_value(&args("/n")).unwrap();
    let caller = Caller::new(AI);
    let stopping = async {
        tokio::time::sleep(Wait::from_millis(300)).await;
        w.engine.stop(&w.stop_proof()).await
    };
    let (verdict, stopped) = tokio::join!(w.engine.revise(&caller, &q.call.id, &again), stopping);
    stopped.unwrap();
    assert_eq!(verdict, Verdict::Refuse(Refusal::Stopped));
}
