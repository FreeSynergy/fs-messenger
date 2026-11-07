//! Asking the source of marks, with a limit, for the object a read names.
//!
//! Pattern: Facade over the Strategy [`SensitivityLookup`]: one fixed way to ask it. The source runs in
//! a task of its own with a time limit. No answer in time, a panic, or `Unknown` all read as marked:
//! in doubt the read asks. Only the engine's source counts; nothing in the call's arguments is looked at.

use std::sync::Arc;

use super::core::GuardEngine;
use crate::no_marks::NoMarks;
use crate::sensitivity::{ObjectRef, SensitivityLookup};

impl GuardEngine {
    /// Whether the read of this object has to ask: marked, not known, or the source failed.
    pub(super) async fn read_asks(&self, object: &ObjectRef) -> bool {
        let source: Arc<dyn SensitivityLookup> = self
            .sensitivity
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(NoMarks));
        let object = object.clone();
        let mut task = tokio::spawn(async move { source.sensitivity(&object).await });
        if let Ok(Ok(answer)) = tokio::time::timeout(self.config.store_wait, &mut task).await {
            return answer.asks();
        }
        task.abort();
        true
    }
}
