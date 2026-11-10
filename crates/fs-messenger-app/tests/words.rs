//! Every key the descriptions and the frontend ask for has an English word.
//!
//! Pattern: none (tests against the public API: `Words`, the registry of the core).

use fs_components_fields::Say;
use fs_messenger_app::{Words, frontend_keys};
use fs_messenger_core::describe::registry;

#[test]
fn every_key_of_the_descriptions_has_a_word() {
    let words = Words::english();
    let r = registry().unwrap();
    let mut missing = Vec::new();
    for d in r.iter() {
        let mut keys = vec![d.label.clone()];
        let mut fields: Vec<_> = d.fields.iter().collect();
        for c in &d.commands {
            keys.push(c.label.clone());
            fields.extend(c.input.iter().chain(c.result.iter()));
        }
        for f in fields {
            keys.push(f.label.clone());
            keys.extend(f.choices.iter().map(|k| format!("{}.{k}", f.label)));
        }
        missing.extend(keys.into_iter().filter(|k| words.lookup(k).is_none()));
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "no word for: {missing:?}");
}

#[test]
fn every_key_of_the_frontend_has_a_word_and_a_missing_key_says_itself() {
    let words = Words::english();
    for k in frontend_keys() {
        assert!(words.lookup(k).is_some(), "{k}");
    }
    assert_eq!(
        words.say("messenger.field.nothing-like-it"),
        "messenger.field.nothing-like-it"
    );
    assert_eq!(
        words.say("hint.messenger.field.to"),
        "",
        "a hint is a placeholder, empty when there is none"
    );
}
