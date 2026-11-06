//! [`Password`]: a login secret that redacts itself.
//!
//! Pattern: Newtype: no `Display`, `Debug` prints a placeholder, the memory is wiped on drop, and the
//! bytes are only readable inside this crate, at the one place that hands them to a transport.

use std::fmt;

use zeroize::Zeroizing;

/// A password or app token. Cannot be printed or read from outside this crate.
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
}
