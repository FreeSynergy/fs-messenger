//! Names that hand out a secret: a rule, not a list (strategy decision 4d(3), 2026-11-07).
//!
//! Pattern: Specification: a pure predicate over a name, with no state and no side effect. The gate
//! asks it before it looks at the invitation or the table, so a new tool that hands out a key cannot
//! be missed by not being listed.
//!
//! The rule, word for word: normalise the name first (Unicode UTS #39: NFKC, lower case, combining marks dropped, the skeleton of
//! confusable characters, lower case again, so `ｔoken`, `tоken` with a Cyrillic o and `TOKEN` read the
//! same); split it at every character that is not a letter or a digit (dot, hyphen, underscore,
//! space); the name is secret if any part **contains** one of `secret`, `token`, `key`, `password`,
//! `passphrase`, `passwd`, `pwd`, `pin`, `seed`, `mnemonic`, `private`, `proof`, `credential`. Containing is deliberate:
//! it catches `apikey`, `apiKey`, `credentials`. CamelCase needs no split of its own for that, because
//! lower-casing makes `apiKey` the part `apikey`. The price is false alarms, by design ("in doubt,
//! shut"): `keyboard`, `monkey`, `turkey`, `keyword`, `proofread` are caught as well. Such a tool is
//! named differently, never released past this rule (open point 11).

/// The words that make a name secret, as parts of a part.
const WORDS: [&str; 13] = [
    "secret",
    "token",
    "key",
    "password",
    "passphrase",
    "passwd",
    "pwd",
    "pin",
    "seed",
    "mnemonic",
    "private",
    "proof",
    "credential",
];

/// The name as the rule compares it: NFKC, lower case, combining marks dropped (`PİN` lower-cases to
/// `i` and a dot mark, which would split the word), confusables folded, lower case again.
fn normalised(name: &str) -> String {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
    use unicode_security::confusable_detection::skeleton;
    let folded: String = name
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|c| !is_combining_mark(*c))
        .collect();
    skeleton(&folded).flat_map(char::to_lowercase).collect()
}

/// The words, folded the same way (the skeleton changes letters: `m` becomes `rn`), once.
fn folded_words() -> &'static [String] {
    static FOLDED: std::sync::LazyLock<Vec<String>> =
        std::sync::LazyLock::new(|| WORDS.iter().map(|word| normalised(word)).collect());
    &FOLDED
}

/// Whether a tool name is one of a secret. Such a tool is for no AI.
pub fn is_secret_name(name: &str) -> bool {
    let words = folded_words();
    normalised(name)
        .split(|c: char| !c.is_alphanumeric())
        .any(|part| words.iter().any(|word| part.contains(word.as_str())))
}

#[cfg(test)]
mod tests {
    use super::is_secret_name;

    #[test]
    fn the_new_words_are_secret() {
        for name in [
            "vault.passwd",
            "user.PWD",
            "card.pin",
            "wallet.seed",
            "wallet.mnemonic",
            "x.private",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn look_alike_spellings_are_secret_too() {
        for name in [
            "account.t\u{043e}ken", // Cyrillic o
            "account.\u{ff54}oken", // fullwidth t
            "account.TOKEN",
            "wallet.P\u{0130}N",      // Turkish dotted capital I
            "account.to\u{0301}ken",  // a combining accent inside the word
            "wallet.mn\u{0435}monic", // Cyrillic e
            "wallet.s\u{0435}ed",
            "vault.pa\u{0455}sword", // Cyrillic s
        ] {
            assert!(is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn plain_names_stay_open() {
        for name in [
            "message.send",
            "conversation.list",
            "draft.list",
            "message.open",
        ] {
            assert!(!is_secret_name(name), "{name}");
        }
    }
}
