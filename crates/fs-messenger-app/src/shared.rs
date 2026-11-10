//! What the four Backends share: the core, the runtime handle, and the small conversions between the Backend's text values
//! and the core's types.
//!
//! Pattern: Adapter (sync `Backend` over the async `Messenger`): `block` runs one call of the core to its end. The window
//! thread is not a runtime thread, so `Handle::block_on` is allowed. Errors of the core become `BackendError` in one place.

use std::future::Future;
use std::sync::Arc;

use fs_admin_frontend::{BackendError, Values};
use fs_messenger_core::{Error, Messenger, describe};
use fs_objects::ObjectId;
use tokio::runtime::Handle;

/// The prefix that makes an object id an IRI (`urn:uuid:`, RFC 8141).
const URN: &str = "urn:uuid:";

/// The core and the runtime that drives it.
pub struct Shared {
    /// The program core.
    pub messenger: Arc<dyn Messenger>,
    handle: Handle,
}

impl Shared {
    /// Share `messenger`, driven on `handle`.
    pub fn new(messenger: Arc<dyn Messenger>, handle: Handle) -> Arc<Self> {
        Arc::new(Self { messenger, handle })
    }

    /// Run one call of the core to its end.
    ///
    /// # Errors
    /// The core's error as a [`BackendError`].
    pub fn block<T>(
        &self,
        call: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, BackendError> {
        self.handle.block_on(call).map_err(fail)
    }

    /// The gate every `run` passes: the command must exist, and the table must say the core carries it out.
    ///
    /// Answers `unsupported:<name>` only for a command that really must not run (the table says no); it is never how a
    /// frontend learns what to show.
    ///
    /// # Errors
    /// `NotFound` for a command that does not exist, `Unavailable("unsupported:<name>")` for one the table marks `false`.
    pub fn gate(&self, data_type: &str, name: &str) -> Result<(), BackendError> {
        match describe::support_of(data_type, name) {
            None => Err(BackendError::NotFound),
            Some((false, _)) => Err(BackendError::Unavailable(format!("unsupported:{name}"))),
            Some((true, _)) => Ok(()),
        }
    }
}

/// A core error as the Backend's error.
pub(crate) fn fail(e: Error) -> BackendError {
    match e {
        Error::NotFound(_) => BackendError::NotFound,
        Error::Unsupported(u) => {
            BackendError::Unavailable(format!("unsupported:{}", u.capability()))
        }
        other => BackendError::Unavailable(other.to_string()),
    }
}

/// The IRI of an object id.
pub(crate) fn iri(id: ObjectId) -> String {
    format!("{URN}{id}")
}

/// An object id from its IRI (or its bare UUID).
pub(crate) fn parse_id(text: &str) -> Result<ObjectId, BackendError> {
    text.parse().map_err(|_| BackendError::NotFound)
}

/// The first value of `key`, if there is a non-empty one.
pub(crate) fn one<'a>(values: &'a Values, key: &str) -> Option<&'a str> {
    values
        .get(key)
        .and_then(|v| v.first())
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
}

/// All non-empty values of `key`.
pub(crate) fn many(values: &Values, key: &str) -> Vec<String> {
    values
        .get(key)
        .map(|v| v.iter().filter(|s| !s.trim().is_empty()).cloned().collect())
        .unwrap_or_default()
}

/// A required value that the frontend has already checked; a missing one is a bug of the caller.
pub(crate) fn need<'a>(values: &'a Values, key: &str) -> Result<&'a str, BackendError> {
    one(values, key).ok_or_else(|| BackendError::Unavailable(format!("missing input: {key}")))
}

/// A required reference, parsed.
pub(crate) fn need_id(values: &Values, key: &str) -> Result<ObjectId, BackendError> {
    parse_id(need(values, key)?)
}

/// `yes` / `no`.
pub(crate) fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Whether the choice is `yes`.
pub(crate) fn is_yes(values: &Values, key: &str) -> bool {
    one(values, key) == Some("yes")
}

/// The media type of a format choice.
pub(crate) fn media_type(values: &Values) -> &'static str {
    if one(values, "mediaType") == Some("markdown") {
        "text/markdown"
    } else {
        "text/plain"
    }
}

/// Put a non-empty value.
pub(crate) fn put(values: &mut Values, key: &str, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty() {
        values.entry(key.to_owned()).or_default().push(value);
    }
}

/// Seconds since the epoch.
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
