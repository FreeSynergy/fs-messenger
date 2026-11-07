//! The first source of marks: it knows none.
//!
//! Pattern: Null object: implements [`SensitivityLookup`] and learns nothing, on purpose. It answers
//! `Unknown` for every object, which counts as marked, so with it every read of an object asks. The
//! real source (`envelope.sensitive` of `fs-store`) replaces it through
//! `GuardEngine::set_sensitivity`.

use async_trait::async_trait;

use crate::sensitivity::{ObjectRef, Sensitivity, SensitivityLookup};

/// Knows no marks.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMarks;

#[async_trait]
impl SensitivityLookup for NoMarks {
    async fn sensitivity(&self, _object: &ObjectRef) -> Sensitivity {
        Sensitivity::Unknown
    }
}
