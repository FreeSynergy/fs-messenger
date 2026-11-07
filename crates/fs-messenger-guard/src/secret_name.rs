//! Names that hand out a secret: a rule, not a list (strategy decision 4d(3), 2026-11-07).
//!
//! Pattern: Specification: a pure predicate over a name, with no state and no side effect. The gate
//! asks it before it looks at the invitation or the table, so a new tool that hands out a key cannot
//! be missed by not being listed.
//!
//! The rule, word for word: split the name at every character that is not a letter or a digit (dot,
//! hyphen, underscore, space); lower-case each part; the name is secret if any part **contains** one of
//! `secret`, `token`, `key`, `password`, `passphrase`, `proof`, `credential`. Containing is deliberate:
//! it catches `apikey`, `apiKey`, `credentials`. CamelCase needs no split of its own for that, because
//! lower-casing makes `apiKey` the part `apikey`. The price is false alarms, by design ("in doubt,
//! shut"): `keyboard`, `monkey`, `turkey`, `keyword`, `proofread` are caught as well. Such a tool is
//! named differently or released by the program's owner in the schema, never by this rule.

/// The words that make a name secret, as parts of a part.
const WORDS: [&str; 7] = [
    "secret",
    "token",
    "key",
    "password",
    "passphrase",
    "proof",
    "credential",
];

/// Whether a tool name is one of a secret. Such a tool is for no AI.
pub fn is_secret_name(name: &str) -> bool {
    name.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .any(|part| WORDS.iter().any(|word| part.contains(word)))
}
