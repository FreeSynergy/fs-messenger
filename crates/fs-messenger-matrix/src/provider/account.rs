//! [`MatrixAccount`]: where an account lives, and what may be refused before anything connects.
//!
//! Pattern: Specification + Value object. A token over plain HTTP is readable on the way, so `http://`
//! passes only for loopback: the local test server, or a tunnel.

use crate::error::MatrixError;

/// Timeline events per room in one `/sync`. More are fetched separately if a room has more.
const DEFAULT_TIMELINE_LIMIT: u32 = 100;

/// Who logs in, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixAccount {
    /// The homeserver's client URL.
    pub homeserver: String,
    /// The Matrix id, `@name:server`. Used as the account's address in the core.
    pub user_id: String,
    /// What other clients see as the device's name.
    pub device_name: String,
    /// Timeline events per room in one `/sync`.
    pub timeline_limit: u32,
}

impl MatrixAccount {
    /// An account with the default limits.
    pub fn new(homeserver: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            homeserver: homeserver.into(),
            user_id: user_id.into(),
            device_name: "FreeSynergy.Messenger".to_owned(),
            timeline_limit: DEFAULT_TIMELINE_LIMIT,
        }
    }

    /// Refuse a homeserver that a token must not be sent to.
    pub(crate) fn check_homeserver(&self) -> Result<(), MatrixError> {
        let url = self.homeserver.as_str();
        if url.starts_with("https://") {
            return Ok(());
        }
        let host = url
            .strip_prefix("http://")
            .ok_or_else(|| MatrixError::Config(format!("not an http(s) URL: {url}")))?
            .split('/')
            .next()
            .unwrap_or_default();
        let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        if matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
            return Ok(());
        }
        Err(MatrixError::Config(format!(
            "plain http only to loopback, not {host}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(url: &str) -> bool {
        MatrixAccount::new(url, "@a:x").check_homeserver().is_ok()
    }

    #[test]
    fn plain_http_is_for_loopback_only() {
        assert!(check("https://matrix.example.org"));
        assert!(check("http://127.0.0.1:18008"));
        assert!(check("http://localhost:8008/"));
        assert!(check("http://[::1]:8008"));
        assert!(!check("http://matrix.example.org"));
        assert!(!check("http://127.0.0.1.example.org:8008"));
        assert!(!check("http://localhost.evil.org"));
        assert!(!check("ftp://127.0.0.1"));
    }
}
