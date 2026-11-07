//! Our own `/sync`, asked from a token of our choosing.
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder):
//! `Client::send` of a `/sync` request, the SDK's `Room::decrypt_event`, and the answer turned into
//! our `Answer`. The SDK does not process this answer: it is only read.

use super::MatrixProvider;
use super::answer::Answer;
use crate::error::Result;

impl MatrixProvider {
    /// `GET /sync?since=...` with the filter of `sync_once`, under `bounded`. Plain events are taken
    /// as they are, encrypted ones go through `Room::decrypt_event` (without a key: a placeholder
    /// that waits).
    pub(super) async fn sync_from(&self, since: Option<&str>) -> Result<Answer> {
        let _ = since;
        unimplemented!("MatrixProvider::sync_from")
    }
}
