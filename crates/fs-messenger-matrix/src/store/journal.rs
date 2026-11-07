//! [`Journal`]: an ordered, atomic log of key-value changes that the stores persist through.
//!
//! Pattern: Strategy: the stores do not know `fs-objects`. One `append` is all or nothing, which is
//! what a store's `save_changes` promises to the SDK: a ratchet step and what depends on it must not
//! come apart in a crash.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::StoreFail;
use crate::secret::SecretText;

/// One change. Keys and values are text; what they mean is the store's business. A value can be key
/// material, so it is a [`SecretText`], and a key must never hold a secret.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Op {
    /// Set a key.
    #[serde(rename = "p")]
    Put(String, SecretText),
    /// Remove a key.
    #[serde(rename = "r")]
    Remove(String),
    /// Remove all keys that start with this.
    #[serde(rename = "x")]
    RemovePrefix(String),
}

/// Apply `ops`, in order, to `map`.
pub(crate) fn apply(map: &mut BTreeMap<String, SecretText>, ops: Vec<Op>) {
    for op in ops {
        match op {
            Op::Put(k, v) => {
                map.insert(k, v);
            }
            Op::Remove(k) => {
                map.remove(&k);
            }
            Op::RemovePrefix(p) => map.retain(|k, _| !k.starts_with(&p)),
        }
    }
}

/// An ordered log of changes that survives a restart.
#[async_trait]
pub trait Journal: Send + Sync {
    /// Write `ops` as one atomic step. When this answers `Ok`, they survive a crash.
    async fn append(&self, ops: Vec<Op>) -> Result<(), StoreFail>;
    /// All keys as they stand now.
    async fn load(&self) -> Result<BTreeMap<String, SecretText>, StoreFail>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_removal_keeps_the_rest() {
        let mut m = BTreeMap::new();
        apply(
            &mut m,
            vec![
                Op::Put("a/1".into(), "x".to_owned().into()),
                Op::Put("a/2".into(), "x".to_owned().into()),
                Op::Put("b".into(), "x".to_owned().into()),
                Op::RemovePrefix("a/".into()),
            ],
        );
        assert_eq!(m.keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn debug_never_shows_a_value() {
        let shown = format!(
            "{:?}",
            Op::Put("k".into(), "secret-value".to_owned().into())
        );
        assert!(!shown.contains("secret-value"));
    }
}
