//! [`ApiKey`]: a credential that redacts itself.
//!
//! Pattern: Newtype: `Debug` and `Display` print a placeholder, it cannot be serialised into an object,
//! the memory is wiped on drop, and the bytes are only readable inside this crate, at the places that
//! put it on a request or look for its echo.

use std::fmt;

use zeroize::Zeroizing;

/// An API key. Cannot be printed or read from outside this crate.
#[derive(Clone)]
pub struct ApiKey(Zeroizing<String>);

impl ApiKey {
    /// Wrap a key.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(Zeroizing::new(secret.into()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neither_debug_nor_display_shows_the_key() {
        let key = ApiKey::new("sk-needle-4711");
        assert!(!format!("{key}").contains("needle"));
        assert!(!format!("{key:?}").contains("needle"));
        assert!(!format!("{:#?}", Some(&key)).contains("needle"));
        assert_eq!(key.expose(), "sk-needle-4711", "the control");
    }
}
