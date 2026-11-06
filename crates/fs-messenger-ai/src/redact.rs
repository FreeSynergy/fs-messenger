//! [`Redactor`]: a server that echoes the API key does not get it stored or shown.
//!
//! Pattern: Strategy (pure text functions). Whatever comes back from a server passes through here before
//! anything keeps it, because the store logs what it gets. It finds the key as text only: an encoded key
//! (base64, split by other characters) is not found, and `DESIGN.md` says so.

use crate::secret::ApiKey;

const MARK: &str = "[redacted]";

/// Takes the key out of text from a server.
#[derive(Clone, Debug, Default)]
pub(crate) struct Redactor {
    key: Option<ApiKey>,
}

impl Redactor {
    pub(crate) fn new(key: Option<ApiKey>) -> Self {
        Self {
            key: key.filter(|k| !k.expose().is_empty()),
        }
    }

    /// `text` without the key, for what arrives whole.
    pub(crate) fn scrubbed(&self, text: &str) -> String {
        match &self.key {
            Some(key) if text.contains(key.expose()) => text.replace(key.expose(), MARK),
            _ => text.to_string(),
        }
    }

    /// How many bytes at the end of `text` could be the start of the key, so they wait for the next
    /// piece: the key can be split over two pieces. Whole code points, since the key's start is.
    pub(crate) fn held_back(&self, text: &str) -> usize {
        let Some(key) = &self.key else {
            return 0;
        };
        let key = key.expose();
        (1..key.len())
            .rev()
            .find(|&n| key.get(..n).is_some_and(|start| text.ends_with(start)))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_goes_and_what_may_be_its_start_waits() {
        let r = Redactor::new(Some(ApiKey::new("sk-key")));
        assert_eq!(r.scrubbed("a sk-key b"), "a [redacted] b");
        assert_eq!(r.held_back("hello sk"), 2);
        assert_eq!(r.held_back("hello s"), 1);
        assert_eq!(r.held_back("hello"), 0);
        assert_eq!(r.held_back("sk-ke"), 5);
    }

    #[test]
    fn without_a_key_nothing_changes() {
        let r = Redactor::new(None);
        assert_eq!(r.scrubbed("sk-key"), "sk-key");
        assert_eq!(r.held_back("sk-key"), 0);
        let empty = Redactor::new(Some(ApiKey::new("")));
        assert_eq!(empty.scrubbed("x"), "x");
    }
}
