//! The type description: the registry accepts it, the capability table covers every id of `requirements.md`, and the
//! table agrees with what the core reports.
//!
//! Pattern: none (tests against the public API: `describe`, `Messenger::capabilities`).

mod common;

use std::collections::BTreeSet;

use fs_messenger_core::Messenger;
use fs_messenger_core::describe::{Delivery, TABLE, commands_of, registry, support_of};
use fs_types::{BasicType, DescribesType};

/// The ids of the snapshot of `requirements.md` (tests/data), one per line.
fn snapshot_ids() -> BTreeSet<String> {
    include_str!("data/requirements-ids.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The ids of the live `requirements.md`, if the documentation repo lies next to this one (a developer machine).
fn live_ids() -> Option<BTreeSet<String>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fs-documentation/docs/en/programs/apps/messenger/requirements.md");
    let text = std::fs::read_to_string(path).ok()?;
    let mut ids = BTreeSet::new();
    for line in text.lines().filter(|l| l.starts_with('|')) {
        for part in line.split('`').skip(1).step_by(2) {
            let is_id = part == "sync"
                || (part.split_once('.').is_some_and(|(a, b)| {
                    !a.is_empty()
                        && a.len() > 1
                        && a.chars().all(|c| c.is_ascii_lowercase())
                        && !b.is_empty()
                        && b.chars().all(|c| c.is_ascii_alphabetic())
                }));
            if is_id {
                ids.insert(part.to_owned());
            }
        }
    }
    Some(ids)
}

fn table_ids() -> BTreeSet<&'static str> {
    TABLE.iter().map(|e| e.capability).collect()
}

#[test]
fn the_table_has_every_capability_id_of_requirements() {
    let have = table_ids();
    let missing: Vec<_> = snapshot_ids()
        .into_iter()
        .filter(|i| !have.contains(i.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "capability ids of requirements.md missing in the table: {missing:?}"
    );
}

#[test]
fn the_table_has_every_id_of_the_live_requirements_when_they_are_next_to_us() {
    let Some(live) = live_ids() else { return };
    let have = table_ids();
    let missing: Vec<_> = live
        .into_iter()
        .filter(|i| !have.contains(i.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "requirements.md has ids that the table lacks: {missing:?}"
    );
}

#[test]
fn the_snapshot_is_not_empty_and_has_the_agent_ids() {
    let s = snapshot_ids();
    assert!(s.len() > 90);
    for id in [
        "toolcall.decide",
        "text.stream",
        "turn.stop",
        "message.send",
        "sync",
    ] {
        assert!(s.contains(id), "{id}");
    }
}

#[test]
fn all_four_types_register_and_validate() {
    let r = registry().expect("descriptions are sound");
    for name in ["Conversation", "Message", "Account", "Draft"] {
        let d = r
            .get(name)
            .unwrap_or_else(|| panic!("{name} is registered"));
        assert_eq!(d.program, "fs-messenger");
        assert!(!d.standard.is_empty());
        assert!(!d.fields.is_empty());
        assert!(
            d.find_command("find").is_some(),
            "{name} has a find command (the list)"
        );
    }
}

#[test]
fn every_command_of_the_table_is_in_the_description_supported_or_not() {
    for e in TABLE {
        if let Delivery::Command { name, .. } = e.delivery {
            let commands = commands_of(e.data_type);
            assert!(
                commands.iter().any(|c| c.name == name),
                "{}.{name} ({})",
                e.data_type,
                e.capability
            );
        }
    }
}

#[test]
fn rows_of_one_command_agree_on_kind_and_support() {
    for a in TABLE {
        for b in TABLE {
            if a.data_type == b.data_type
                && a.command_name().is_some()
                && a.command_name() == b.command_name()
            {
                assert_eq!(
                    a.supported, b.supported,
                    "{} / {}",
                    a.capability, b.capability
                );
                let (Delivery::Command { kind: ka, .. }, Delivery::Command { kind: kb, .. }) =
                    (a.delivery, b.delivery)
                else {
                    unreachable!()
                };
                assert_eq!(ka, kb, "{} / {}", a.capability, b.capability);
            }
        }
    }
}

#[test]
fn an_unsupported_row_says_why_and_a_supported_one_does_not() {
    for e in TABLE {
        assert_eq!(e.supported, e.reason.is_empty(), "{}", e.capability);
    }
}

#[test]
fn a_field_row_names_a_field_of_its_type() {
    let r = registry().unwrap();
    for e in TABLE {
        if let Delivery::Field(f) = e.delivery {
            assert!(
                r.get(e.data_type).unwrap().find_field(f).is_some(),
                "{} -> {f}",
                e.capability
            );
        }
    }
}

#[test]
fn a_reference_names_a_type_that_exists() {
    let r = registry().unwrap();
    for name in ["Conversation", "Message", "Account", "Draft"] {
        let d = r.get(name).unwrap();
        let all = d.fields.iter().chain(
            d.commands
                .iter()
                .flat_map(|c| c.input.iter().chain(c.result.iter())),
        );
        for f in all.filter(|f| f.basic == BasicType::Reference) {
            assert!(
                r.get(&f.target).is_some(),
                "{name}: {} -> {}",
                f.name,
                f.target
            );
        }
    }
}

/// Core can do it, but this app cannot offer it through the generated frontend.
const CORE_CAN_APP_CANNOT: [&str; 5] = [
    "event.subscribe",
    "attachment.add",
    "attachment.save",
    "toolcall.decide",
    "turn.stop",
];

#[tokio::test]
async fn the_table_agrees_with_what_the_core_reports() {
    let (repo, _store) = common::setup().await;
    let report = repo.capabilities();
    let missing: Vec<_> = report
        .entries()
        .iter()
        .filter(|s| !TABLE.iter().any(|e| e.capability == s.capability))
        .map(|s| s.capability.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "the core reports ids that the table lacks: {missing:?}"
    );
    for s in report.entries() {
        let rows: Vec<_> = TABLE
            .iter()
            .filter(|e| e.capability == s.capability)
            .collect();
        if CORE_CAN_APP_CANNOT.contains(&s.capability.as_str()) {
            assert!(rows.iter().all(|r| !r.supported), "{}", s.capability);
        } else {
            assert!(
                rows.iter().all(|r| r.supported == s.supported),
                "{}: core says {}",
                s.capability,
                s.supported
            );
        }
    }
}

#[test]
fn support_of_answers_by_type_and_command() {
    assert_eq!(support_of("Message", "create").map(|s| s.0), Some(true));
    assert_eq!(support_of("Message", "react").map(|s| s.0), Some(false));
    assert!(support_of("Message", "nothing").is_none());
}

#[test]
fn describe_is_the_same_on_every_call() {
    assert_eq!(
        fs_messenger_core::Account::describe(),
        fs_messenger_core::Account::describe()
    );
}
