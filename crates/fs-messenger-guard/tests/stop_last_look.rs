//! The last look at the latch is right before `Allow`, after every slow step (log, save, display).

mod common;

use std::sync::atomic::Ordering;

use common::AI;
use common::program::{Fixed, object};
use common::rigs::Rig;
use fs_messenger_core::{MessageAction, Messenger as _};
use fs_messenger_guard::{Arguments, Caller, Gate, Guard, Method, Refusal, Sensitivity, Verdict};

#[tokio::test]
async fn a_stop_during_the_log_of_a_class_allow_is_not_answered_with_allow() {
    let r = Rig::open().await;
    // A read is allowed by its class only when it names an object the engine knows to be unmarked.
    assert!(r.engine.set_sensitivity(Fixed::new(Sensitivity::Unmarked)));
    r.faulty.hold_append.store(true, Ordering::SeqCst);
    let (engine, input) = (
        r.engine.clone(),
        Arguments::from_text(r#"{"path":"/"}"#).unwrap(),
    );
    let checking = tokio::spawn(async move {
        let caller = Caller::new(AI);
        engine
            .check_object(&caller, "message.open", &input, &object())
            .await
    });
    r.faulty.entered.notified().await;
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    r.faulty.go.notify_one();
    assert_eq!(checking.await.unwrap(), Verdict::Refuse(Refusal::Stopped));
}

#[tokio::test]
async fn a_stop_during_the_log_of_a_used_approval_is_not_answered_with_allow() {
    let r = Rig::open().await;
    let input = Arguments::from_text(r#"{"path":"/m"}"#).unwrap();
    let caller = Caller::new(AI);
    let Verdict::Ask(q) = r.engine.check(&caller, "message.delete", &input).await else {
        panic!()
    };
    let d = r
        .owner
        .decide(&r.identity, q.message, &q.call, "allow-once");
    r.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    r.faulty.hold_append.store(true, Ordering::SeqCst);
    let engine = r.engine.clone();
    let checking =
        tokio::spawn(async move { engine.check(&caller, "message.delete", &input).await });
    r.faulty.entered.notified().await;
    r.engine.stop(&r.proof(Method::Stop)).await.unwrap();
    r.faulty.go.notify_one();
    assert_eq!(checking.await.unwrap(), Verdict::Refuse(Refusal::Stopped));
}
