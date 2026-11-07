//! F3 of the review of 9ea2830 (strategy 4d(3), 2026-11-07): a rule, not a list. A tool name with a
//! part `secret`, `token`, `key`, `password`, `passphrase`, `proof` or `credential` is for no AI,
//! in the table or not, and before the invitation is looked at.

mod common;

use common::{AI, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{
    CapabilityRule, Refusal, Verdict, capability_rule, is_secret_name, provisional_table,
};

const SECRET: &[&str] = &[
    "account.secret",
    "account.token",
    "account.password",
    "account.passphrase",
    "attachment.key",
    "x.apiKey",
    "x.APIKEY",
    "a.b.Credential",
    "account.credentials",
    "device.proof",
    "client-secret.read",
    "my_token.get",
    "Account.Token",
];

#[test]
fn names_with_a_secret_part_are_secret_in_any_spelling() {
    for name in SECRET {
        assert!(is_secret_name(name), "{name}");
    }
}

#[test]
fn plain_names_are_not_secret() {
    for name in [
        "message.open",
        "conversation.list",
        "draft.save",
        "account.list",
        "turn.stop",
    ] {
        assert!(!is_secret_name(name), "{name}");
    }
}

/// The known false alarm of "in doubt, shut": a part that merely contains a word counts.
#[test]
fn a_part_that_contains_a_word_counts_which_is_a_known_false_alarm() {
    for name in ["keyboard.layout", "monkey.list"] {
        assert!(is_secret_name(name), "{name}");
    }
}

#[test]
fn no_entry_of_the_table_is_caught_by_accident() {
    for (id, rule) in provisional_table() {
        if matches!(rule, CapabilityRule::Risk(_)) {
            assert!(!is_secret_name(id), "{id} is in the table as a plain tool");
        }
    }
}

#[test]
fn the_rule_makes_it_never_for_an_ai() {
    for name in SECRET {
        assert_eq!(capability_rule(name), CapabilityRule::NeverForAi, "{name}");
        assert_eq!(
            capability_rule(&format!("  {name} ")),
            CapabilityRule::NeverForAi
        );
    }
}

/// Before the invitation: someone not invited is told `NeverForAi` too, not `NotInvited`.
#[tokio::test]
async fn a_secret_name_is_refused_with_everything_released_and_before_the_invitation() {
    let w = World::with_ai().await;
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    w.release(AI, R::Changes, "changes").await;
    for name in SECRET {
        assert_eq!(
            w.check(name, args("/")).await,
            Verdict::Refuse(Refusal::NeverForAi),
            "{name}"
        );
    }
    let stranger = fs_messenger_guard::Caller::new("https://local.invalid/ai/stranger");
    let arguments = fs_messenger_guard::Arguments::from_value(&args("/")).unwrap();
    let verdict =
        fs_messenger_guard::Gate::check(&*w.engine, &stranger, "account.token", &arguments).await;
    assert_eq!(verdict, Verdict::Refuse(Refusal::NeverForAi));
}
