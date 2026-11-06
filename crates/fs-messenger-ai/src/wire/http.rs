//! [`Transport`]: one origin, no redirects, no proxy, bounded in time and size.
//!
//! Pattern: Facade over `reqwest`, and Template method for the attempts of one request. The credential
//! goes to the configured origin only: redirects are refused, not followed, and the environment's proxy
//! settings are ignored, so text meant for a loopback address does not leave by a proxy. Retries end when
//! any answer began; a `retry-after` over the cap ends the turn at once instead of sitting it out.

use std::future::Future;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::redirect::Policy;
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};

use super::dialect::short_name;
use crate::config::ModelConfig;
use crate::error::AiError;
use crate::limits::Limits;
use crate::turn::TurnFailure;

/// At most this much of an error's body is read, for its `error.type`.
const ERROR_BODY_BYTES: usize = 64 * 1024;

/// What can end a turn from outside while it waits: a stop, the idle limit, the turn limit.
pub(crate) struct Control {
    cancel: watch::Receiver<bool>,
    deadline: Instant,
    idle: Duration,
}

impl Control {
    pub(crate) fn new(cancel: watch::Receiver<bool>, limits: &Limits) -> Self {
        Self {
            cancel,
            deadline: Instant::now() + limits.turn,
            idle: limits.idle,
        }
    }

    /// `future`, unless a stop or a limit comes first. A stop wins over what is ready, and dropping
    /// `future` here is what closes the connection.
    pub(crate) async fn bounded<T>(
        &mut self,
        future: impl Future<Output = T>,
    ) -> Result<T, TurnFailure> {
        let idle_at = Instant::now() + self.idle;
        let (at, late) = if idle_at < self.deadline {
            (idle_at, TurnFailure::IdleTimeout)
        } else {
            (self.deadline, TurnFailure::TurnTimeout)
        };
        // Also when the sender has gone: nobody can say "go on" any more.
        tokio::select! {
            biased;
            _ = self.cancel.wait_for(|stop| *stop) => Err(TurnFailure::Stopped),
            done = timeout_at(at, future) => done.map_err(|_| late),
        }
    }

    /// Wait, unless a stop or the turn limit comes first.
    pub(crate) async fn sleep(&mut self, wait: Duration) -> Result<(), TurnFailure> {
        let at = (Instant::now() + wait).min(self.deadline);
        tokio::select! {
            biased;
            _ = self.cancel.wait_for(|stop| *stop) => Err(TurnFailure::Stopped),
            () = tokio::time::sleep_until(at) => {
                if at == self.deadline {
                    Err(TurnFailure::TurnTimeout)
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// The way to one origin.
pub(crate) struct Transport {
    http: reqwest::Client,
    limits: Limits,
}

fn may_retry(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504 | 529)
}

/// Seconds, as documented. A date or anything else counts as not said.
fn retry_after(response: &reqwest::Response) -> Option<u64> {
    let value = response.headers().get(RETRY_AFTER)?.to_str().ok()?;
    value.trim().parse().ok()
}

fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
}

impl Transport {
    pub(crate) fn new(config: &ModelConfig) -> Result<Self, AiError> {
        let uses_tls = config.endpoint.uses_tls();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(config.trust.client_config(uses_tls)?)
            .https_only(uses_tls)
            .redirect(Policy::none())
            .no_proxy()
            .connect_timeout(config.limits.idle)
            .build()
            .map_err(|e| AiError::Tls(e.without_url().to_string()))?;
        Ok(Self {
            http,
            limits: config.limits.clone(),
        })
    }

    /// The stream, after up to `retries` more attempts.
    pub(crate) async fn open(
        &self,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
        body: Vec<u8>,
        control: &mut Control,
    ) -> Result<reqwest::Response, TurnFailure> {
        if body.len() > self.limits.request_bytes {
            return Err(TurnFailure::RequestTooLarge);
        }
        let mut attempt = 0;
        loop {
            let mut request = self.http.post(url).body(body.clone());
            request = request.header(CONTENT_TYPE, "application/json");
            for (name, value) in headers {
                request = request.header(name.clone(), value.clone());
            }
            let answer = control.bounded(request.send()).await?;
            let (failure, named_wait) = match answer {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        if !is_event_stream(&response) {
                            return Err(TurnFailure::WrongContentType);
                        }
                        return Ok(response);
                    }
                    if status.is_redirection() {
                        return Err(TurnFailure::Redirected(status.as_u16()));
                    }
                    let wait = retry_after(&response);
                    let failure = Self::refusal(response, control).await?;
                    if !may_retry(status) {
                        return Err(failure);
                    }
                    (failure, wait)
                }
                // Only what never reached the server is safe to send again: a model bills for the rest.
                Err(e) if e.is_connect() => (TurnFailure::Connection, None),
                Err(_) => return Err(TurnFailure::BrokeOff),
            };
            if attempt >= self.limits.retries {
                return Err(failure);
            }
            let wait = self.wait_for(attempt, named_wait)?;
            control.sleep(wait).await?;
            attempt += 1;
        }
    }

    fn wait_for(&self, attempt: u32, named: Option<u64>) -> Result<Duration, TurnFailure> {
        match named {
            Some(seconds) if Duration::from_secs(seconds) > self.limits.retry_after_cap => {
                Err(TurnFailure::RateLimited {
                    wait_seconds: seconds,
                })
            }
            Some(seconds) => Ok(Duration::from_secs(seconds)),
            None => Ok(self.limits.backoff.saturating_mul(1 << attempt.min(8))),
        }
    }

    /// What an error status says: its code and a short name, never its text.
    async fn refusal(
        mut response: reqwest::Response,
        control: &mut Control,
    ) -> Result<TurnFailure, TurnFailure> {
        let status = response.status().as_u16();
        let mut body = vec![];
        while let Ok(Some(chunk)) = control.bounded(response.chunk()).await? {
            body.extend_from_slice(&chunk);
            if body.len() > ERROR_BODY_BYTES {
                break;
            }
        }
        let parsed: Option<serde_json::Value> = serde_json::from_slice(&body).ok();
        let word = parsed
            .as_ref()
            .and_then(|v| v.pointer("/error/type"))
            .and_then(serde_json::Value::as_str);
        Ok(TurnFailure::Http {
            status,
            kind: short_name(word),
        })
    }
}
