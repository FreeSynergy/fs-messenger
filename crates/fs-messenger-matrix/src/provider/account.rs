//! [`MatrixAccount`]: where an account lives, and what may be refused before anything connects.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). A
//! token over plain HTTP is readable on the way, so `http://` passes only for loopback: the local
//! test server, or a tunnel. The URL may hold a password (user info), so it is named in no error and
//! `Debug` shows only its origin.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use url::{Host, Url};

use crate::error::MatrixError;

/// Timeline events per room in one `/sync`. More are fetched separately if a room has more.
const DEFAULT_TIMELINE_LIMIT: u32 = 100;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Who logs in, and where.
#[derive(Clone, PartialEq, Eq)]
pub struct MatrixAccount {
    /// The homeserver's client URL.
    pub homeserver: String,
    /// The Matrix id, `@name:server`. Used as the account's address in the core.
    pub user_id: String,
    /// What other clients see as the device's name.
    pub device_name: String,
    /// Timeline events per room in one `/sync`.
    pub timeline_limit: u32,
    /// How long one request may take, and one read of its answer may stall.
    pub request_timeout: Duration,
}

impl fmt::Debug for MatrixAccount {
    // Never the URL itself: a refused one may hold user info with a password.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let origin = Url::parse(&self.homeserver).ok();
        let origin = origin.map(|u| u.origin().ascii_serialization());
        f.debug_struct("MatrixAccount")
            .field("homeserver", &origin.as_deref().unwrap_or("<not a URL>"))
            .field("user_id", &self.user_id)
            .field("device_name", &self.device_name)
            .field("timeline_limit", &self.timeline_limit)
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}

impl MatrixAccount {
    /// An account with the default limits.
    pub fn new(homeserver: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            homeserver: homeserver.into(),
            user_id: user_id.into(),
            device_name: "FreeSynergy.Messenger".to_owned(),
            timeline_limit: DEFAULT_TIMELINE_LIMIT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// The homeserver's origin (scheme, host, port) in one spelling, as a token is bound to it.
    pub(crate) fn origin(&self) -> Result<String, MatrixError> {
        self.check_homeserver()?;
        let url = Url::parse(&self.homeserver).map_err(|_| MatrixError::Config("no URL".into()))?;
        Ok(url.origin().ascii_serialization())
    }

    /// Refuse a homeserver that a token must not be sent to: not http(s), with user info (the part
    /// before `@` is not the host), or plain `http://` to anything but exactly `localhost`,
    /// `127.0.0.1` or `[::1]`. The URL is taken apart by the `url` crate, as the SDK does.
    pub(crate) fn check_homeserver(&self) -> Result<(), MatrixError> {
        let text = self.homeserver.as_str();
        // Not the URL: it may hold a password.
        let refuse = |why: &str| Err(MatrixError::Config(why.to_owned()));
        if text.contains('\\') {
            return refuse("a backslash in the homeserver URL");
        }
        let Ok(url) = Url::parse(text) else {
            return refuse("not a valid http(s) URL");
        };
        if !url.username().is_empty() || url.password().is_some() {
            return refuse("the homeserver URL holds user info");
        }
        let Some(host) = url.host() else {
            return refuse("no host in the homeserver URL");
        };
        match url.scheme() {
            "https" => Ok(()),
            "http" if is_loopback(&host) => Ok(()),
            "http" => refuse("plain http only to loopback"),
            _ => refuse("not an http(s) URL"),
        }
    }
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => *name == "localhost",
        Host::Ipv4(ip) => *ip == Ipv4Addr::LOCALHOST,
        Host::Ipv6(ip) => *ip == Ipv6Addr::LOCALHOST,
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

    #[test]
    fn the_real_host_decides_not_the_text_before_a_colon() {
        for url in [
            "http://localhost:80@evil.org",
            "http://localhost@evil.org",
            "http://127.0.0.1:8008@evil.org/",
            "http://localhost%40evil.org",
            "http://localhost:80%40evil.org",
            "http://localhost\\@evil.org",
            "http://evil.org\\@localhost",
            "http://evil.org#@localhost",
            "http://evil.org?@localhost",
            "http://user:pw@localhost:8008",
            "http://localhost.evil.org:8008",
            "http://127.0.0.1.evil.org",
            "http://[::1].evil.org",
            "http://localhost:notaport",
            "http://localhost:99999",
            "http://",
            "https://user@matrix.example.org",
            "https://user:pw@matrix.example.org",
            "https://matrix.example.org\\@evil.org",
            "https://",
        ] {
            assert!(!check(url), "must be refused: {url}");
        }
    }

    #[test]
    fn loopback_in_any_case_and_with_any_port() {
        for url in [
            "http://LOCALHOST:8008",
            "http://LocalHost",
            "http://localhost:1",
            "http://127.0.0.1",
            "http://127.0.0.1:65535/_matrix",
            "http://[::1]",
            "http://[::1]:8008/x",
            "https://Matrix.Example.org:8448/path",
        ] {
            assert!(check(url), "must pass: {url}");
        }
    }
}
