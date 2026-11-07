//! Without an explicit release the guard refuses and asks nobody (ADR 0054, "Ohne Befugnis nichts").
//! A release holds for the participant, the class and the program, and for nothing else.

mod common;

use common::{AI, OTHER_AI, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{Allowance, Refusal, Verdict};

const DENIED: Verdict = Verdict::Refuse(Refusal::NotReleased);

#[tokio::test]
async fn no_release_means_no_for_every_class_and_no_question() {
    let w = World::with_ai().await;
    let before = w.record().await;
    let logged = w.engine.log().await.unwrap().len();
    for tool in [
        "message.open",   // readsOnly
        "message.flag",   // changes
        "message.delete", // destroys
        "message.send",   // actsForPerson
        "shell.run",      // not in the table: unknown
        "message.act",    // unknown by table
    ] {
        assert_eq!(w.check(tool, args("/")).await, DENIED, "{tool}");
    }
    assert_eq!(w.record().await, before, "a refusal writes nothing");
    assert_eq!(
        w.engine.log().await.unwrap().len(),
        logged,
        "nor a log entry"
    );
}

#[tokio::test]
async fn what_always_asks_asks_once_something_is_released_in_that_program() {
    let w = World::with_ai().await;
    assert_eq!(w.check("message.delete", args("/m")).await, DENIED);
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    w.ask("message.delete", args("/m")).await;
    w.ask("message.send", args("/m")).await;
    // A class that is not released stays refused, also with another class released.
    assert_eq!(w.check("message.flag", args("/")).await, DENIED);
}

#[tokio::test]
async fn a_release_holds_for_its_participant_only() {
    let w = World::with_ai().await;
    w.invite(OTHER_AI).await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    assert!(matches!(
        w.read("message.open", args("/")).await,
        Verdict::Allow(Allowance::Class(R::ReadsOnly))
    ));
    let other = fs_messenger_guard::Arguments::from_value(&args("/")).unwrap();
    let verdict = fs_messenger_guard::Gate::check(
        &*w.engine,
        &fs_messenger_guard::Caller::new(OTHER_AI),
        "message.open",
        &other,
    )
    .await;
    assert_eq!(verdict, DENIED);
}

#[tokio::test]
async fn a_release_holds_for_its_class_only() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    assert_eq!(w.check("message.flag", args("/")).await, DENIED);
    w.release(AI, R::Changes, "changes").await;
    assert!(matches!(
        w.check("message.flag", args("/")).await,
        Verdict::Allow(Allowance::Class(R::Changes))
    ));
}

#[tokio::test]
async fn a_release_holds_for_its_program_only() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await; // program "message"
    assert!(matches!(
        w.read("message.open", args("/")).await,
        Verdict::Allow(_)
    ));
    // Same class, other programs: refused. `account.list` and `conversation.list` are readsOnly.
    assert_eq!(w.read("account.list", args("/")).await, DENIED);
    assert_eq!(w.read("conversation.list", args("/")).await, DENIED);
    // What always asks needs a release in its own program too.
    assert_eq!(w.check("account.remove", args("/")).await, DENIED);
    w.release_in(AI, R::ReadsOnly, "account").await.unwrap();
    assert!(matches!(
        w.read("account.list", args("/")).await,
        Verdict::Allow(_)
    ));
    w.ask("account.remove", args("/")).await;
    assert_eq!(w.read("conversation.list", args("/")).await, DENIED);
}

#[tokio::test]
async fn the_program_is_the_part_of_the_name_before_the_first_dot() {
    assert_eq!(common::program::program("message.open"), "message");
    assert_eq!(common::program::program("Message.Open.More"), "message");
    assert_eq!(common::program::program(" account.list "), "account");
    assert_eq!(common::program::program("solo"), "solo");
}
