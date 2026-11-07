//! Release per participant and per class; the brake asks every time, whatever is stored.

mod common;

use common::{AI, OTHER_AI, World, args};
use fs_messenger_core::{ToolCallChoice, ToolCallRisk as R};
use fs_messenger_guard::{
    AiGrant, Allowance, Arguments, Caller, Gate, Guard, GuardError, Refusal, Verdict,
    can_be_released, offers_allow_class,
};
use serde_json::json;

#[tokio::test]
async fn release_refuses_what_can_never_be_granted_and_changes_nothing() {
    let w = World::working().await;
    for (risk, name) in [
        (R::Unknown, "unknown"),
        (R::Destroys, "destroys"),
        (R::ActsForPerson, "actsForPerson"),
    ] {
        let before = w.record().await;
        let grant = AiGrant {
            profile: AI.into(),
            risk,
            program: "message".into(),
            goes_to: "this device only".into(),
        };
        let refused = w.engine.release(&grant, &w.release_proof(AI, name)).await;
        assert!(
            matches!(refused, Err(GuardError::Refused(Refusal::NotReleasable(r))) if r == risk),
            "{name}: {refused:?}"
        );
        assert_eq!(w.record().await, before, "{name} changed something");
    }
}

#[tokio::test]
async fn a_release_needs_an_invited_participant() {
    let w = World::open().await;
    let grant = AiGrant {
        profile: AI.into(),
        risk: R::ReadsOnly,
        program: "message".into(),
        goes_to: "this device only".into(),
    };
    let refused = w
        .engine
        .release(&grant, &w.release_proof(AI, "readsOnly"))
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotInvited))
    ));
}

#[tokio::test]
async fn a_release_holds_per_class_and_per_participant() {
    let w = World::with_ai().await;
    w.invite(OTHER_AI).await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Allow(Allowance::Class(R::ReadsOnly))
    );
    assert_eq!(
        w.check("message.flag", args("/")).await,
        Verdict::Refuse(Refusal::NotReleased),
        "another class is refused, not asked"
    );
    let other = w
        .engine
        .check(
            &Caller::new(OTHER_AI),
            "message.open",
            &Arguments::from_value(&args("/")).unwrap(),
        )
        .await;
    assert_eq!(
        other,
        Verdict::Refuse(Refusal::NotReleased),
        "another participant is refused, not asked"
    );
}

#[tokio::test]
async fn revoke_takes_the_releases_of_this_participant_only() {
    let w = World::working().await;
    w.invite(OTHER_AI).await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    w.release(OTHER_AI, R::ReadsOnly, "readsOnly").await;
    let proof = w.owner.proof(
        &w.identity,
        fs_messenger_guard::Method::Revoke,
        &[AI.as_bytes(), b""],
    );
    w.engine.revoke(AI, &proof).await.unwrap();
    let state = w.engine.state().await;
    assert_eq!(
        state.grants,
        [AiGrant {
            profile: OTHER_AI.into(),
            risk: R::ReadsOnly,
            program: "message".into(),
            goes_to: "this device only".into(),
        }]
    );
    assert_eq!(state.invited.len(), 2, "still invited");
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
}

#[tokio::test]
async fn the_brake_asks_for_what_destroys_or_acts_whatever_is_stored() {
    let w = World::working().await;
    // The record is tampered with: each class is "released", also the ones that can never be.
    w.tamper(|doc| {
        doc["grants"] = json!([
            { "profile": AI, "risk": "unknown" },
            { "profile": AI, "risk": "destroys" },
            { "profile": AI, "risk": "actsForPerson" },
            { "profile": AI, "risk": "someNewClass" },
        ]);
    })
    .await;
    for capability in [
        "message.delete",
        "message.send",
        "message.forward",
        "not.in.the.table",
    ] {
        let q = w.ask(capability, args("/x")).await;
        assert_eq!(q.call.options.len(), 2, "{capability}: no release on offer");
    }
}

#[tokio::test]
async fn unknown_is_what_the_table_does_not_know_and_a_caller_cannot_help() {
    let w = World::working().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    w.release(AI, R::Changes, "changes").await;
    let q = w.ask("sneaky.read", args("/x")).await;
    assert_eq!(q.call.risk, R::Unknown);
    let choices: Vec<_> = q.call.options.iter().map(|o| o.choice).collect();
    assert_eq!(choices, [ToolCallChoice::Refuse, ToolCallChoice::AllowOnce]);
}

#[test]
fn allow_class_is_only_on_offer_for_reads_only_and_changes() {
    for risk in [R::ReadsOnly, R::Changes] {
        assert!(can_be_released(risk));
        assert!(offers_allow_class(risk, true));
        assert!(!offers_allow_class(risk, false), "not without input");
    }
    for risk in [R::Unknown, R::Destroys, R::ActsForPerson] {
        assert!(!can_be_released(risk));
        assert!(!offers_allow_class(risk, true));
    }
}

#[tokio::test]
async fn a_question_offers_allow_class_according_to_its_risk() {
    let w = World::working().await;
    let has_class = |q: &fs_messenger_guard::Question| {
        q.call
            .options
            .iter()
            .any(|o| o.choice == ToolCallChoice::AllowClass)
    };
    // Releasable classes are not asked at all any more: released by a person, or refused.
    assert_eq!(
        w.check("message.open", args("/a")).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
    assert!(matches!(
        w.check("message.flag", args("/b")).await,
        Verdict::Allow(_)
    ));
    assert!(!has_class(&w.ask("message.delete", args("/c")).await));
    assert!(!has_class(&w.ask("message.send", args("/d")).await));
    assert!(!has_class(&w.ask("what.is.this", args("/e")).await));
}

#[tokio::test]
async fn a_stored_risk_of_a_call_has_no_say() {
    let w = World::working().await;
    let q = w.ask("message.delete", args("/m")).await;
    w.tamper(|doc| {
        // A record cannot talk the engine out of the brake: the risk is not even stored.
        doc["calls"][0]["risk"] = json!("readsOnly");
        doc["calls"][0]["capability"] = json!("message.delete");
    })
    .await;
    let again = w.ask("message.delete", args("/m")).await;
    assert_eq!(again.call.id, q.call.id);
    assert_eq!(again.call.risk, R::Destroys);
    assert_eq!(again.call.options.len(), 2);
}

#[tokio::test]
async fn the_guard_itself_is_never_for_an_ai_and_no_person_is_asked() {
    let w = World::working().await;
    for capability in [
        "toolcall.decide",
        "guard.stop",
        "guard.reopen",
        "guard.release",
        "GUARD.reopen",
        " guard.invite",
        "messenger.guard",
        "account.secret",
    ] {
        assert_eq!(
            w.check(capability, args("/")).await,
            Verdict::Refuse(Refusal::NeverForAi),
            "{capability}"
        );
    }
}

#[tokio::test]
async fn a_tool_name_that_could_hide_text_from_the_person_is_refused() {
    let w = World::working().await;
    for name in ["", "x\ny", "message.open\u{202e}eteled", "a\u{200b}b"] {
        assert_eq!(
            w.check(name, args("/")).await,
            Verdict::Refuse(Refusal::BadName),
            "{name:?}"
        );
    }
}
