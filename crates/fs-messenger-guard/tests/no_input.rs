//! A call without arguments: digest over "no input", risk `unknown`, `allowOnce` only; the canonical form.

mod common;

use common::{AI, World, args};
use fs_messenger_core::{MessageAction, Messenger, ToolCallRisk};
use fs_messenger_guard::{Allowance, Arguments, Caller, Gate, Guard, Verdict, arguments_digest};
use serde_json::json;
#[tokio::test]
async fn without_arguments_the_digest_is_still_there_risk_unknown_and_allow_once_only() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let none = Arguments::none();
    let call = |args: &Arguments| {
        let (e, a) = (w.engine.clone(), args.clone());
        async move { e.check(&Caller::new(AI), "message.open", &a).await }
    };
    // Released for this class, and still: no input, no release counts.
    let Verdict::Ask(q) = call(&none).await else {
        panic!()
    };
    assert_eq!(q.call.risk, ToolCallRisk::Unknown);
    assert_eq!(
        q.call.arguments_digest,
        arguments_digest(AI, "message.open", &none)
    );
    assert!(!q.call.arguments_digest.is_empty());
    let ids: Vec<_> = q.call.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["refuse", "allow-once"]);
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-class");
    assert!(
        w.repo
            .act(q.conversation, q.message, MessageAction::Decide(d))
            .await
            .is_err()
    );
    assert_eq!(w.engine.state().await.grants.len(), 1, "nothing was added");
}

#[tokio::test]
async fn later_input_changes_the_digest_so_a_release_lapses() {
    let w = World::with_ai().await;
    let none = Arguments::none();
    let Verdict::Ask(q) = w
        .engine
        .check(&Caller::new(AI), "message.delete", &none)
        .await
    else {
        panic!()
    };
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    // The input arrives after the approval: not what the person approved.
    let input = Arguments::from_value(&args("/etc/passwd")).unwrap();
    assert!(matches!(
        w.engine
            .check(&Caller::new(AI), "message.delete", &input)
            .await,
        Verdict::Ask(_)
    ));
    // "No input" itself is what was approved, once.
    assert!(matches!(
        w.engine
            .check(&Caller::new(AI), "message.delete", &none)
            .await,
        Verdict::Allow(Allowance::Once { .. })
    ));
}

#[test]
fn no_input_and_an_empty_object_are_different_values() {
    let none = Arguments::none();
    let empty = Arguments::from_value(&json!({})).unwrap();
    assert_ne!(
        arguments_digest(AI, "t", &none),
        arguments_digest(AI, "t", &empty)
    );
    assert_eq!(
        Arguments::from_value(&serde_json::Value::Null).unwrap(),
        none
    );
    assert_eq!(Arguments::from_text("null").unwrap(), none);
}

#[test]
fn the_digest_covers_caller_tool_and_canonical_arguments_and_cannot_be_shifted() {
    let a = Arguments::from_text(r#"{"a":1,"b":[true,{"y":2,"x":3}]}"#).unwrap();
    let b = Arguments::from_text(r#"{ "b": [true, {"x":3, "y":2}], "a":1 }"#).unwrap();
    assert_eq!(a, b, "key order and white space do not count");
    assert_eq!(
        a.canonical().unwrap(),
        r#"{"a":1,"b":[true,{"x":3,"y":2}]}"#
    );
    let d = arguments_digest(AI, "t", &a);
    assert_eq!(d.len(), 32);
    assert_ne!(d, arguments_digest("other", "t", &a));
    assert_ne!(d, arguments_digest(AI, "other", &a));
    assert_ne!(
        d,
        arguments_digest(AI, "t", &Arguments::from_text("[1]").unwrap())
    );
    assert_ne!(
        arguments_digest("ab", "c", &a),
        arguments_digest("a", "bc", &a)
    );
}

#[test]
fn what_two_programs_could_read_differently_is_refused() {
    assert!(Arguments::from_text(r#"{"path":"/safe","path":"/"}"#).is_err());
    assert!(Arguments::from_text(r#"{"a":{"x":1,"x":2}}"#).is_err());
    assert!(Arguments::from_text(r#"[{"x":1,"x":2}]"#).is_err());
    assert!(Arguments::from_text("not json").is_err());
    let big = json!({ "x": "y".repeat(70 * 1024) });
    assert!(
        Arguments::from_value(&big).is_err(),
        "never cut off, so never over the limit"
    );
    let mut deep = json!(1);
    for _ in 0..80 {
        deep = json!([deep]);
    }
    assert!(Arguments::from_value(&deep).is_err());
}

#[tokio::test]
async fn what_is_shown_is_the_whole_input_and_cannot_hide_text() {
    let w = World::with_ai().await;
    let q = w.ask("shell.run", json!({ "c": "ok\u{202e}fr- mr" })).await;
    assert!(q.call.detail.contains("\\u{202e}"), "{}", q.call.detail);
    assert!(!q.call.detail.contains('\u{202e}'));
    assert_eq!(w.shown(&q).await.detail, q.call.detail);
    assert_eq!(q.call.summary, "", "agent text does not go here");
}

#[test]
fn the_digest_is_exactly_these_bytes_with_an_explicit_no_input() {
    use sha2::{Digest, Sha256};
    let hash = |fields: &[&[u8]]| {
        let mut h = Sha256::new();
        for f in fields {
            h.update((f.len() as u64).to_be_bytes());
            h.update(f);
        }
        h.finalize().to_vec()
    };
    let context: &[u8] = b"FreeSynergy.Messenger tool call digest v1";
    assert_eq!(
        arguments_digest("me", "t", &Arguments::none()),
        hash(&[context, b"me", b"t", b"no input"])
    );
    assert_eq!(
        arguments_digest("me", "t", &Arguments::from_text("[1]").unwrap()),
        hash(&[context, b"me", b"t", b"input", b"[1]"])
    );
}
