//! ADR 0054, "Lesen ist nicht harmlos": secrets never reach an AI, and reporting outward is acting
//! for the person. What the table can say about this today, and what it cannot.

mod common;

use common::{AI, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{CapabilityRule, Refusal, Verdict, capability_rule, provisional_table};

#[test]
fn everything_that_sends_something_out_is_acting_for_the_person() {
    for tool in [
        "message.send",
        "message.reply",
        "message.schedule",
        "message.forward",
        "message.react",
        "message.edit",
        "conversation.markRead",
        "rule.write",
        "account.add",
    ] {
        assert_eq!(
            capability_rule(tool),
            CapabilityRule::Risk(R::ActsForPerson),
            "{tool}"
        );
    }
    // A name that looks like reporting or sending and is not in the table is `unknown`, which asks.
    for tool in [
        "message.report",
        "message.sendOut",
        "report.send",
        "mail.send",
    ] {
        assert_eq!(
            capability_rule(tool),
            CapabilityRule::Risk(R::Unknown),
            "{tool}"
        );
    }
}

#[test]
fn nothing_that_sends_or_reports_can_be_released_as_a_class() {
    for (name, rule) in provisional_table() {
        let sends = ["send", "reply", "schedule", "forward", "report"]
            .iter()
            .any(|word| name.ends_with(word));
        if sends {
            assert_eq!(*rule, CapabilityRule::Risk(R::ActsForPerson), "{name}");
        }
    }
}

#[tokio::test]
async fn the_secret_tool_is_never_for_an_ai_in_any_spelling() {
    let w = World::working().await;
    for spelling in ["account.secret", "Account.Secret", " account.secret "] {
        assert_eq!(
            w.check(spelling, args("/")).await,
            Verdict::Refuse(Refusal::NeverForAi),
            "{spelling}"
        );
    }
    let _ = AI;
}

/// The limit, stated: the gate knows a secret tool only by the exact name `account.secret`. Another
/// name for a tool that hands out a key or a token is `unknown` here; with no release in its program
/// it is refused, and with one it asks, but it is not `NeverForAi`. See `DESIGN.md`, open points.
#[tokio::test]
async fn another_name_for_a_secret_is_only_unknown_to_the_gate() {
    for tool in ["account.token", "account.credentials", "attachment.key"] {
        assert_eq!(
            capability_rule(tool),
            CapabilityRule::Risk(R::Unknown),
            "{tool}"
        );
    }
    let w = World::with_ai().await;
    assert_eq!(
        w.check("account.token", args("/")).await,
        Verdict::Refuse(Refusal::NotReleased)
    );
}
