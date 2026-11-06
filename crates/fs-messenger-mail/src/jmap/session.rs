//! The JMAP session document (RFC 8620 section 2), and what we let it say.
//!
//! Pattern: Value object + Specification. A session document is server input, and it decides where our
//! password goes next. So every URL in it must be `https`, and with `rebase` it is moved onto the origin
//! of the session URL the person configured. The test server, for one, answers over HTTPS and advertises
//! `http://`; that is refused, not followed.

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

fn checked(url: String, session_url: &str, rebase: bool) -> Result<String> {
    let url = if rebase {
        rebased(&url, session_url)?
    } else {
        url
    };
    if url.starts_with("https://") {
        Ok(url)
    } else {
        Err(MailError::Jmap(format!(
            "the session advertises {url}, which is not https; refused, the password stays here"
        )))
    }
}

impl Session {
    /// Read and check the document.
    pub(super) fn read(body: &[u8], session_url: &str, rebase: bool) -> Result<Self> {
        let d: Document = serde_json::from_slice(body)?;
        Ok(Self {
            api_url: checked(d.api_url, session_url, rebase)?,
            upload_url: checked(d.upload_url, session_url, rebase)?,
            download_url: checked(d.download_url, session_url, rebase)?,
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

    #[test]
    fn plain_http_in_the_document_is_refused() {
        let e = Session::read(DOC, "https://mail.example.org/jmap/session", false).unwrap_err();
        assert!(e.to_string().contains("not https"));
    }

    #[test]
    fn rebasing_keeps_the_configured_origin() {
        let s = Session::read(DOC, "https://mail.example.org:8443/jmap/session", true).unwrap();
        assert_eq!(s.api_url, "https://mail.example.org:8443/jmap/");
        assert_eq!(s.account, "c");
        assert!(s.upload_url.ends_with("/jmap/upload/{accountId}/"));
    }
}
