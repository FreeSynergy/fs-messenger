//! [`Password`] and [`SecretText`]: secrets that redact themselves.
//!
//! Pattern: Newtype: no `Display`, `Debug` prints a placeholder, the memory is wiped on drop, and the
//! bytes are only readable inside this crate, at the places that hand them to the SDK or the store.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

/// A password or passphrase. Cannot be printed or read from outside this crate.
#[derive(Clone)]
pub struct Password(Zeroizing<String>);

impl Password {
    /// Wrap a secret.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(Zeroizing::new(secret.into()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

/// A journal value: a pickled Olm or Megolm key, an access token, or anything else a store keeps.
/// Treated as secret whatever it holds, so that no code has to decide which one is.
#[derive(Clone)]
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// Whether it holds exactly this. For tests of a store; it shows nothing itself.
    pub fn matches(&self, text: &str) -> bool {
        self.expose() == text
    }
}

impl From<String> for SecretText {
    fn from(text: String) -> Self {
        Self(Zeroizing::new(text))
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

impl Serialize for SecretText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_secret() {
        let p = Password::new("hunter2-very-secret");
        assert!(!format!("{p:?}").contains("hunter2"));
        assert!(!format!("{:#?}", Some(&p)).contains("hunter2"));
        assert_eq!(p.expose(), "hunter2-very-secret");
    }

    #[test]
    fn a_journal_value_shows_nothing_and_stays_text_in_a_store() {
        let v = SecretText::from("olm-pickle-needle".to_owned());
        assert!(!format!("{v:?} {:#?}", Some(&v)).contains("needle"));
        let stored = serde_json::to_string(&v).unwrap();
        assert_eq!(stored, "\"olm-pickle-needle\"");
        let back: SecretText = serde_json::from_str(&stored).unwrap();
        assert!(back.matches("olm-pickle-needle"));
    }
}
