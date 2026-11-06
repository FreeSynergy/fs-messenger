//! What the review named before the tag: spending in another conversation, numbers, floods, invitations.

mod common;

use common::{AI, World, args};
use fs_messenger_core::{MessageAction, Messenger};
use fs_messenger_guard::{Allowance, Arguments, ArgumentsError, Caller, Gate, Refusal, Verdict};
use serde_json::json;
#[tokio::test]
async fn an_allow_once_cannot_be_spent_in_another_conversation() {
    let w = World::with_ai().await;
    let account = w.account("me@example.org", "mail").await;
    w.deliver(account.id, "m1", "x@y").await.unwrap();
    let here = w.repo.conversations(None, false, None).await.unwrap().items[0].id;
    let input = Arguments::from_value(&args("/m")).unwrap();
    let in_here = Caller::in_conversation(AI, here);
    let Verdict::Ask(q) = w.engine.check(&in_here, "message.delete", &input).await else {
        panic!()
    };
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();

    for elsewhere in [
        Caller::new(AI),
        Caller::in_conversation(AI, w.repo_conversation("m2").await),
    ] {
        w.pause();
        let verdict = w.engine.check(&elsewhere, "message.delete", &input).await;
        assert!(!matches!(verdict, Verdict::Allow(_)), "{verdict:?}");
    }
    assert!(matches!(
        w.engine.check(&in_here, "message.delete", &input).await,
        Verdict::Allow(Allowance::Once { .. })
    ));
}

#[test]
fn numbers_that_have_no_one_meaning_are_refused() {
    for text in [
        "18446744073709551616",
        "18446744073709551617",
        "9007199254740992",
        "-9007199254740992",
        "9007199254740993",
        r#"{"n":18446744073709551616}"#,
        "[1e300]",
        "0.1000000000000000055",
        "1.00000000000000000001",
    ] {
        assert!(
            matches!(
                Arguments::from_text(text),
                Err(ArgumentsError::NumberOutOfRange(_))
            ),
            "{text}"
        );
    }
    assert!(Arguments::from_value(&json!(1u64 << 53)).is_err());
    assert!(Arguments::from_value(&json!(1e300)).is_err());
    for fine in [
        "9007199254740991",
        "-1",
        "0.1",
        "1.5e3",
        "0",
        "-0.5",
        r#""18446744073709551616""#,
    ] {
        assert!(Arguments::from_text(fine).is_ok(), "{fine}");
    }
    // Inside a string nothing is a number, escaped quotes included.
    assert!(Arguments::from_text(r#"["\" 18446744073709551616 \""]"#).is_ok());
}

#[tokio::test]
async fn revise_has_a_limit_and_does_not_keep_a_card_alive() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", args("a")).await;
    let caller = Caller::new(AI);
    // At once: refused, and nothing is written.
    let input = Arguments::from_value(&args("b")).unwrap();
    let before = w.record().await;
    assert_eq!(
        w.engine.revise(&caller, &q.call.id, &input).await,
        Verdict::Refuse(Refusal::RetryTooSoon)
    );
    assert_eq!(w.record().await, before);
    // With pauses: only so many revisions; the limit is in the state, so a restart does not refill it.
    let mut last = Verdict::Refuse(Refusal::Stopped);
    for n in 0..40 {
        let input = Arguments::from_value(&args(&format!("/{n}"))).unwrap();
        last = w.pause().engine.revise(&caller, &q.call.id, &input).await;
    }
    assert_eq!(last, Verdict::Refuse(Refusal::RetriesExhausted));
    assert!(w.shown(&q).await.revision <= 16);

    let q2 = w.ask("shell.run", args("c")).await;
    let expires = w.record().await.unwrap()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == q2.call.id.as_str())
        .unwrap()["expiresAt"]
        .clone();
    let input = Arguments::from_value(&args("d")).unwrap();
    assert!(matches!(
        w.pause().engine.revise(&caller, &q2.call.id, &input).await,
        Verdict::Ask(_)
    ));
    let after = w.record().await.unwrap()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == q2.call.id.as_str())
        .unwrap()["expiresAt"]
        .clone();
    assert_eq!(expires, after, "new input does not extend it");
}

#[tokio::test]
async fn expiry_and_conversation_stop_do_not_refill_without_end() {
    let w = World::with_ai().await;
    for n in 0..16 {
        w.ask("message.delete", args(&format!("/{n}"))).await;
    }
    let a = w.ask("message.delete", args("/0")).await;
    // The window of 16 refills through stop, but the hour does not.
    let mut asked = 16;
    let verdict = loop {
        w.repo.stop(a.conversation).await.unwrap();
        w.pause();
        let mut last = None;
        for n in 0..8 {
            let v = w
                .check("message.delete", args(&format!("/{asked}-{n}")))
                .await;
            asked += 1;
            if matches!(v, Verdict::Refuse(_)) {
                last = Some(v);
                break;
            }
        }
        if let Some(v) = last {
            break v;
        }
        assert!(asked < 100, "no end");
    };
    assert_eq!(verdict, Verdict::Refuse(Refusal::TooManyAsked));
    w.clock.advance(chrono::Duration::minutes(61));
    assert!(matches!(
        w.check("message.delete", args("/later")).await,
        Verdict::Ask(_)
    ));
}

#[tokio::test]
async fn settled_questions_are_pruned_with_their_notes() {
    let w = World::with_ai().await;
    let first = w.ask("message.delete", args("/first")).await;
    w.repo.stop(first.conversation).await.unwrap();
    let mut n = 0;
    for _ in 0..6 {
        for _ in 0..16 {
            w.ask("message.delete", args(&format!("/{n}"))).await;
            n += 1;
        }
        w.repo.stop(first.conversation).await.unwrap();
        w.clock.advance(chrono::Duration::minutes(61));
    }
    w.ask("message.delete", args("/last")).await;
    let gone = w.repo.message(first.conversation, first.message).await;
    assert!(gone.is_err(), "the oldest note went");
    let settled = w.record().await.unwrap()["calls"].as_array().unwrap().len();
    assert!(settled <= 64 + 16, "{settled}");
}

#[test]
fn hidden_characters_are_escaped_in_what_a_person_reads() {
    for c in [
        '\u{3164}',
        '\u{115F}',
        '\u{1160}',
        '\u{FFA0}',
        '\u{2800}',
        '\u{034F}',
        '\u{FE0F}',
        '\u{E0100}',
        '\u{180B}',
        '\u{17B4}',
        '\u{1D173}',
        '\u{202E}',
    ] {
        let hidden = format!("{{\"c\":\"a{c}b\"}}");
        let args = Arguments::from_text(&hidden).unwrap();
        let shown = args.detail();
        assert!(!shown.contains(c), "{:x}", c as u32);
        assert!(
            shown.contains(&format!("\\u{{{:x}}}", c as u32)),
            "{:x}",
            c as u32
        );
    }
}
