//! The JMAP session document (RFC 8620 section 2), and what we let it say.
//!
//! Pattern: Value object + Specification. A session document is server input, and it decides where our
//! password goes next. So every URL in it must be `https` and on the origin the person configured, or
//! on an origin the account was explicitly told about; with `rebase` it is moved onto the configured
//! origin. The test server, for one, answers over HTTPS and advertises `http://`; that is refused unless
//! rebased, not followed.

use serde::Deserialize;

use crate::error::{MailError, Result};

const MAIL: &str = "urn:ietf:params:jmap:mail";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    api_url: String,
    upload_url: String,
    download_url: String,
    primary_accounts: std::collections::HashMap<String, String>,
}

/// Where to talk to, checked.
#[derive(Clone, Debug)]
pub(super) struct Session {
    pub(super) api_url: String,
    pub(super) upload_url: String,
    pub(super) download_url: String,
    pub(super) account: String,
}

fn origin_end(url: &str) -> Option<usize> {
    let start = url.find("://")? + 3;
    Some(url[start..].find('/').map_or(url.len(), |i| start + i))
}

/// `url` with the scheme, host and port of `session_url`.
fn rebased(url: &str, session_url: &str) -> Result<String> {
    let bad = || MailError::Jmap("the session document has a malformed URL".into());
    let path = &url[origin_end(url).ok_or_else(bad)?..];
    let origin = &session_url[..origin_end(session_url).ok_or_else(bad)?];
    Ok(format!("{origin}{path}"))
}

/// Scheme, host and port, in lower case.
fn origin_of(url: &str) -> Option<String> {
    origin_end(url).map(|end| url[..end].to_lowercase())
}

/// How far a session document's URLs are believed. They decide where our password goes next.
#[derive(Clone, Copy, Debug)]
pub(super) struct Origins<'a> {
    /// The session URL the person configured.
    pub(super) session_url: &'a str,
    /// Move every URL onto its origin.
    pub(super) rebase: bool,
    /// Other `https` origins this account was explicitly told to send its credentials to.
    pub(super) others: &'a [String],
}

fn checked(url: String, origins: Origins<'_>) -> Result<String> {
    let url = if origins.rebase {
        rebased(&url, origins.session_url)?
    } else {
        url
    };
    if !url.starts_with("https://") {
        return Err(MailError::Jmap(format!(
            "the session advertises {url}, which is not https; refused, the password stays here"
        )));
    }
    let same = |other: &str| origin_of(&url).is_some_and(|o| Some(o) == origin_of(other));
    if same(origins.session_url) || origins.others.iter().any(|o| same(o)) {
        Ok(url)
    } else {
        Err(MailError::Jmap(format!(
            "the session advertises {url}, another origin than the configured one; refused, the \
             password stays here"
        )))
    }
}

impl Session {
    /// Read and check the document.
    pub(super) fn read(body: &[u8], origins: Origins<'_>) -> Result<Self> {
        let d: Document = serde_json::from_slice(body)?;
        Ok(Self {
            api_url: checked(d.api_url, origins)?,
            upload_url: checked(d.upload_url, origins)?,
            download_url: checked(d.download_url, origins)?,
            account: d
                .primary_accounts
                .get(MAIL)
                .cloned()
                .ok_or_else(|| MailError::Jmap("no primary mail account".into()))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &[u8] = br#"{"apiUrl":"http://inner:8080/jmap/","uploadUrl":"http://inner:8080/jmap/upload/{accountId}/",
"downloadUrl":"http://inner:8080/jmap/download/{accountId}/{blobId}/{name}?accept={type}",
"primaryAccounts":{"urn:ietf:params:jmap:mail":"c"}}"#;
    const ELSEWHERE: &[u8] =
        br#"{"apiUrl":"https://evil.example/jmap/","uploadUrl":"https://mail.example.org/u/",
"downloadUrl":"https://mail.example.org/d/","primaryAccounts":{"urn:ietf:params:jmap:mail":"c"}}"#;
    const URL: &str = "https://mail.example.org/jmap/session";

    fn origins<'a>(rebase: bool, others: &'a [String]) -> Origins<'a> {
        Origins {
            session_url: URL,
            rebase,
            others,
        }
    }

    #[test]
    fn plain_http_in_the_document_is_refused() {
        let e = Session::read(DOC, origins(false, &[])).unwrap_err();
        assert!(e.to_string().contains("not https"));
    }

    #[test]
    fn rebasing_keeps_the_configured_origin() {
        let url = "https://mail.example.org:8443/jmap/session";
        let o = Origins {
            session_url: url,
            rebase: true,
            others: &[],
        };
        let s = Session::read(DOC, o).unwrap();
        assert_eq!(s.api_url, "https://mail.example.org:8443/jmap/");
        assert_eq!(s.account, "c");
        assert!(s.upload_url.ends_with("/jmap/upload/{accountId}/"));
    }

    #[test]
    fn another_origin_gets_no_credentials_unless_the_account_says_so() {
        let e = Session::read(ELSEWHERE, origins(false, &[])).unwrap_err();
        assert!(e.to_string().contains("another origin"), "{e}");
        let wrong = ["https://mail.example.org.evil.example".to_string()];
        assert!(Session::read(ELSEWHERE, origins(false, &wrong)).is_err());
        let allowed = ["https://EVIL.example".to_string()];
        let s = Session::read(ELSEWHERE, origins(false, &allowed)).unwrap();
        assert_eq!(s.api_url, "https://evil.example/jmap/");
    }

    #[test]
    fn a_user_part_in_a_url_is_another_origin() {
        let doc = br#"{"apiUrl":"https://mail.example.org@evil.example/","uploadUrl":"https://mail.example.org/u",
"downloadUrl":"https://mail.example.org/d","primaryAccounts":{"urn:ietf:params:jmap:mail":"c"}}"#;
        assert!(Session::read(doc, origins(false, &[])).is_err());
    }
}
