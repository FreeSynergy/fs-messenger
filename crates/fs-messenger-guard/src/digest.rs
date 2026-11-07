//! The arguments digest: what exactly a decision is about.
//!
//! Pattern: none (several pure functions, more than the single one that section 2 allows for `none`; no
//! pattern fits; reported to the strategy, DESIGN.md open point 0). Free functions: SHA-256 over a fixed context, the caller, the
//! conversation of its turn, the tool name and the canonical arguments, each with its length in front,
//! so nothing can move from one field to another. No input is hashed as its own explicit value, so `{}` and "no input" differ and later input changes
//! the digest. Keys are not secret, and nothing here is: the digest is shown to the person's device.

use fs_store::ObjectId;
use sha2::{Digest, Sha256};

use crate::arguments::Arguments;

const DIGEST_CONTEXT: &[u8] = b"FreeSynergy.Messenger tool call digest v1";
const NO_CONVERSATION: &[u8] = b"no conversation";
const CONVERSATION: &[u8] = b"conversation";
const NO_INPUT: &[u8] = b"no input";
const INPUT: &[u8] = b"input";

fn field(hash: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    hash.update(length.to_be_bytes());
    hash.update(bytes);
}

/// The digest of one call. The conversation of the turn is in it, so an `allowOnce` given in one
/// conversation cannot be spent in another.
pub fn arguments_digest(
    caller: &str,
    conversation: Option<ObjectId>,
    tool: &str,
    arguments: &Arguments,
) -> Vec<u8> {
    let mut hash = Sha256::new();
    field(&mut hash, DIGEST_CONTEXT);
    field(&mut hash, caller.as_bytes());
    match conversation {
        None => field(&mut hash, NO_CONVERSATION),
        Some(id) => {
            field(&mut hash, CONVERSATION);
            field(&mut hash, id.to_string().as_bytes());
        }
    }
    field(&mut hash, tool.as_bytes());
    match arguments.canonical() {
        None => field(&mut hash, NO_INPUT),
        Some(text) => {
            field(&mut hash, INPUT);
            field(&mut hash, text.as_bytes());
        }
    }
    hash.finalize().to_vec()
}

/// SHA-256 with a label, for the single-use keys.
pub(crate) fn single_use_key(label: &[u8], parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    field(&mut hash, label);
    for part in parts {
        field(&mut hash, part);
    }
    hex(&hash.finalize())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            // Infallible on a String.
            let _ = write!(s, "{b:02x}");
            s
        })
}

pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}
