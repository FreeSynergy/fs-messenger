//! The words of the program: the English key table (ADR 0057, coding.md 2a).
//!
//! Pattern: Registry (key to text) behind the `Say` strategy of `fs-components-fields`. All English text of this
//! program is in this file; translation replaces the table, not its users. Code never decides by comparing these words.
//!
//! The keys come from three places: the frontend (`FRONTEND`), the descriptions of `fs-messenger-core`
//! (`messenger.type.*`, `messenger.field.*`, `messenger.command.<type>.<command>`, and `<field label>.<choice>`), and the
//! problem codes of `fs-types`. A command label is looked up by its command (the same word in every type, `COMMANDS`);
//! a choice by its last part (`CHOICES`). A missing key says itself, as `Say` demands; a missing `hint.` is empty (a hint
//! is a placeholder: the key would only show as noise).

use fs_components_fields::Say;

use super::choices::CHOICES;
use super::commands::COMMANDS;
use super::frontend::FRONTEND;
use super::labels::LABELS;

/// The English words.
#[derive(Clone, Copy, Debug, Default)]
pub struct Words;

fn find(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

impl Words {
    /// The English table.
    #[must_use]
    pub fn english() -> Self {
        Self
    }

    /// The text of `key`, if the table has it.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&'static str> {
        if let Some(t) = find(FRONTEND, key).or_else(|| find(LABELS, key)) {
            return Some(t);
        }
        if let Some(rest) = key.strip_prefix("messenger.command.") {
            // `<type>.<command in kebab case>`
            let (_, command) = rest.split_once('.')?;
            return COMMANDS
                .iter()
                .find(|(c, _)| kebab(c) == command)
                .map(|(_, v)| *v);
        }
        if key.starts_with("messenger.field.") {
            // `<field label>.<choice>`
            let (_, choice) = key.rsplit_once('.')?;
            return find(CHOICES, choice);
        }
        None
    }
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Say for Words {
    fn say(&self, key: &str) -> String {
        match self.lookup(key) {
            Some(t) => t.to_owned(),
            None if key.starts_with("hint.") => String::new(),
            None => key.to_owned(),
        }
    }
}

/// The keys the generated frontend asks for with a fixed name (for the test that every one has a word).
#[must_use]
pub fn frontend_keys() -> Vec<&'static str> {
    FRONTEND.iter().map(|(k, _)| *k).collect()
}
