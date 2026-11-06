//! [`JmapClient`]: the session, method calls, upload and download.
//!
//! Pattern: Facade over `reqwest`. Small on purpose: `jmap-client` builds its HTTP client inside and has
//! only a general "accept invalid certificates" switch for a self-signed server, which we must not use,
//! and it follows the session's URLs as they are. No redirects either: they could carry the password
//! elsewhere. Only `https`, enforced by the client itself.

use std::time::Duration;

use reqwest::redirect::Policy;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use super::session::Session;
use crate::error::{MailError, Result};
use crate::secret::Password;
use crate::tls::TlsTrust;

const CORE: &str = "urn:ietf:params:jmap:core";
const TIMEOUT: Duration = Duration::from_secs(60);

/// How to reach a JMAP server.
#[derive(Clone, Debug)]
pub struct JmapConfig {
    /// The session URL, `https` only.
    pub session_url: String,
    /// Login name.
    pub user: String,
    /// Login secret.
    pub password: Password,
    /// Whose certificates count.
    pub trust: TlsTrust,
    /// Use the origin of `session_url` for every URL the session names, for a server behind a proxy
    /// that advertises its inner address. Off: the session's URLs must be `https` by themselves.
    pub rebase_on_session_origin: bool,
}

/// A method answered with an error (RFC 8620 section 3.6.2).
#[derive(Debug)]
pub(super) struct MethodError {
    pub(super) kind: String,
}

/// Talks JMAP to one account.
pub struct JmapClient {
    config: JmapConfig,
    http: reqwest::Client,
    session: OnceCell<Session>,
}

pub(super) fn err(e: impl std::fmt::Display) -> MailError {
    MailError::Jmap(e.to_string())
}

impl JmapClient {
    /// A client for `config`. Connects on use.
    pub fn new(config: JmapConfig) -> Result<Self, MailError> {
        if !config.session_url.starts_with("https://") {
            return Err(err("the session URL must be https"));
        }
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(config.trust.client_config()?)
            .https_only(true)
            .redirect(Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(err)?;
        Ok(Self {
            config,
            http,
            session: OnceCell::new(),
        })
    }

    fn authed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.config.user, Some(self.config.password.expose()))
    }

    fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(err(format!("HTTP {status}")))
        }
    }

    pub(super) async fn session(&self) -> Result<&Session> {
        self.session
            .get_or_try_init(|| async {
                let url = &self.config.session_url;
                let response = self.authed(self.http.get(url)).send().await.map_err(err)?;
                let body = Self::checked(response)?.bytes().await.map_err(err)?;
                Session::read(&body, url, self.config.rebase_on_session_origin)
            })
            .await
    }

    /// Run method calls. Answers the `methodResponses`, in order.
    pub(super) async fn call(&self, using: &[&str], calls: Value) -> Result<Vec<Value>> {
        let url = self.session().await?.api_url.clone();
        let mut capabilities = vec![CORE];
        capabilities.extend_from_slice(using);
        let body = json!({ "using": capabilities, "methodCalls": calls });
        let response = self
            .authed(self.http.post(url))
            .json(&body)
            .send()
            .await
            .map_err(err)?;
        let answer: Value = Self::checked(response)?.json().await.map_err(err)?;
        match answer.get("methodResponses") {
            Some(Value::Array(list)) => Ok(list.clone()),
            _ => Err(err("the answer has no methodResponses")),
        }
    }

    /// Upload a blob; answers its id.
    pub(super) async fn upload(&self, bytes: Vec<u8>) -> Result<String> {
        let session = self.session().await?;
        let url = session.upload_url.replace("{accountId}", &session.account);
        let response = self
            .authed(self.http.post(url))
            .header(reqwest::header::CONTENT_TYPE, "message/rfc822")
            .body(bytes)
            .send()
            .await
            .map_err(err)?;
        let answer: Value = Self::checked(response)?.json().await.map_err(err)?;
        answer["blobId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| err("the upload answered no blobId"))
    }

    /// Download a blob, byte for byte.
    pub(super) async fn download(&self, blob: &str) -> Result<Vec<u8>> {
        let session = self.session().await?;
        let url = session
            .download_url
            .replace("{accountId}", &session.account)
            .replace("{blobId}", blob)
            .replace("{name}", "mail.eml")
            .replace("{type}", "message/rfc822");
        let response = self.authed(self.http.get(url)).send().await.map_err(err)?;
        let body = Self::checked(response)?.bytes().await.map_err(err)?;
        Ok(body.to_vec())
    }

    /// The arguments of the response with call id `id`, or the method's error.
    pub(super) fn response<'a>(
        responses: &'a [Value],
        id: &str,
    ) -> Result<std::result::Result<&'a Value, MethodError>> {
        let hit = responses
            .iter()
            .find(|r| r[2] == id)
            .ok_or_else(|| err(format!("no response for call {id}")))?;
        if hit[0] == "error" {
            return Ok(Err(MethodError {
                kind: hit[1]["type"].as_str().unwrap_or("unknown").to_string(),
            }));
        }
        Ok(Ok(&hit[1]))
    }

    /// As [`JmapClient::response`], with a method's error as an error.
    pub(super) fn answer<'a>(responses: &'a [Value], id: &str) -> Result<&'a Value> {
        Self::response(responses, id)?.map_err(|e| err(format!("call {id} answered {}", e.kind)))
    }
}
