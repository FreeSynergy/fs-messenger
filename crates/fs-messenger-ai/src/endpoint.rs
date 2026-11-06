//! [`Endpoint`]: where a model is, and [`DataPath`]: what that means for the person's text.
//!
//! Pattern: Newtype as proof: an `Endpoint` exists only if it needs no secret of its own, names one
//! origin and nothing else, and uses TLS, unless it is a literal loopback address. A name such as
//! `localhost` counts as remote: a resolver decides where it goes, and "does not leave the device" must
//! not hang on a resolver.

use std::net::IpAddr;

use crate::error::AiError;

/// One origin: scheme, host, port. Credentials go here and nowhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    origin: String,
    tls: bool,
    loopback: bool,
}

/// Where the text of a turn goes. A frontend shows this before a person sends (assistant page: "Where
/// data goes").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataPath {
    /// To a literal loopback address: it stays on this device, in the server that runs there.
    ThisDevice,
    /// It leaves this device, over TLS, and whoever runs this origin can read it.
    LeavesDevice {
        /// Who gets it.
        origin: String,
    },
}

fn bad(why: &str) -> AiError {
    AiError::Endpoint(why.to_string())
}

fn is_loopback(host: &str) -> bool {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

impl Endpoint {
    /// Read an origin such as `https://models.example.org` or `http://127.0.0.1:8081`.
    pub fn parse(origin: &str) -> Result<Self, AiError> {
        let url = reqwest::Url::parse(origin).map_err(|_| bad("not a URL"))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(bad("credentials do not belong in the URL"));
        }
        if url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
            return Err(bad("an origin only: no path, query or fragment"));
        }
        let host = url.host_str().ok_or_else(|| bad("no host"))?;
        let loopback = is_loopback(host);
        let tls = match url.scheme() {
            "https" => true,
            "http" if loopback => false,
            "http" => return Err(bad("TLS is required except on a literal loopback address")),
            _ => return Err(bad("only http and https")),
        };
        let origin = url.origin().ascii_serialization();
        Ok(Self {
            origin,
            tls,
            loopback,
        })
    }

    /// `scheme://host:port`.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Whether the way there is protected by TLS.
    pub fn uses_tls(&self) -> bool {
        self.tls
    }

    /// Where text sent here goes.
    pub fn data_path(&self) -> DataPath {
        if self.loopback {
            DataPath::ThisDevice
        } else {
            DataPath::LeavesDevice {
                origin: self.origin.clone(),
            }
        }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_on_a_literal_loopback_address() {
        for good in [
            "http://127.0.0.1:8081",
            "http://127.9.9.9",
            "http://[::1]:1",
        ] {
            let e = Endpoint::parse(good).unwrap();
            assert_eq!(e.data_path(), DataPath::ThisDevice, "{good}");
            assert!(!e.uses_tls());
        }
        for bad in [
            "http://example.org",
            "http://localhost:8081",
            "http://10.0.0.1",
            "http://[::ffff:127.0.0.1]",
            "ftp://127.0.0.1",
        ] {
            assert!(Endpoint::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_name_counts_as_remote_even_if_it_resolves_to_here() {
        let e = Endpoint::parse("https://localhost:1").unwrap();
        assert!(matches!(e.data_path(), DataPath::LeavesDevice { .. }));
        assert!(e.uses_tls());
    }

    #[test]
    fn an_origin_and_nothing_else() {
        for bad in [
            "https://u:p@example.org",
            "https://u@example.org",
            "https://example.org/v1",
            "https://example.org/?a=1",
            "https://example.org/#a",
            "nonsense",
        ] {
            assert!(Endpoint::parse(bad).is_err(), "{bad}");
        }
        let e = Endpoint::parse("https://example.org/").unwrap();
        assert_eq!(e.url("/v1/messages"), "https://example.org/v1/messages");
    }
}
