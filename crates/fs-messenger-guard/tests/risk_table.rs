//! The table: every capability the program reports has an entry, and one entry per union member.

mod common;

use common::World;
use fs_messenger_core::{MessageAction, Messenger, ToolCallDecision, ToolCallRisk as R};
use fs_messenger_guard::{CapabilityRule, action_capability, capability_rule, provisional_table};

#[test]
fn what_is_not_in_the_table_is_unknown() {
    for id in ["", "message", "message.delete ", "MESSAGE.DELETE", "x.y"] {
        assert_eq!(
            capability_rule(id),
            CapabilityRule::Risk(R::Unknown),
            "{id:?}"
        );
    }
}

#[test]
fn the_brake_classes_of_the_table() {
    use CapabilityRule::Risk;
    for (id, risk) in [
        ("message.open", R::ReadsOnly),
        ("message.flag", R::Changes),
        ("message.delete", R::Destroys),
        ("account.remove", R::Destroys),
        ("message.send", R::ActsForPerson),
        ("message.forward", R::ActsForPerson),
        ("message.react", R::ActsForPerson),
        ("message.edit", R::ActsForPerson),
        ("rule.write", R::ActsForPerson),
        ("message.act", R::Unknown),
    ] {
        assert_eq!(capability_rule(id), Risk(risk), "{id}");
    }
}

#[test]
fn every_member_of_the_act_union_has_its_own_entry() {
    let actions = [
        MessageAction::None,
        MessageAction::Delete,
        MessageAction::Flag(true),
        MessageAction::Unread(true),
        MessageAction::React(String::new()),
        MessageAction::Forward(vec![]),
        MessageAction::Edit(String::new()),
        MessageAction::Decide(ToolCallDecision::default()),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for action in &actions {
        let id = action_capability(action);
        assert!(provisional_table().iter().any(|(t, _)| *t == id), "{id}");
        assert!(seen.insert(id), "{id} twice");
    }
    assert_eq!(
        capability_rule(action_capability(&MessageAction::Decide(
            ToolCallDecision::default()
        ))),
        CapabilityRule::NeverForAi
    );
}

#[tokio::test]
async fn every_capability_the_program_reports_is_in_the_table() {
    let w = World::fresh().await;
    let report = w.repo.capabilities();
    let missing: Vec<_> = report
        .entries()
        .iter()
        .map(|s| s.capability.as_str())
        .filter(|id| !provisional_table().iter().any(|(t, _)| t == id))
        .collect();
    assert!(missing.is_empty(), "not in the table: {missing:?}");
}
