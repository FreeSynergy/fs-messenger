//! Text helpers: preview and display name.
//!
//! Pattern: Strategy (pure functions), so the same rule holds for ingest, send and edit.

use super::consts::{TEXT_MARKDOWN, TEXT_PLAIN};

const PREVIEW_CHARS: usize = 140;
const NAME_CHARS: usize = 80;
const MAX_INDEXED_TEXT: usize = 256 * 1024;

fn shortened(text: &str, chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(chars).collect()
}

/// The contract knows two body types; anything else is shown as what it can safely be.
pub(super) fn body_media_type(given: &str) -> String {
    if given == TEXT_MARKDOWN {
        TEXT_MARKDOWN
    } else {
        TEXT_PLAIN
    }
    .to_string()
}

/// What search sees of a body: its start, cut on a character. Provisional, see `DESIGN.md`.
pub(super) fn indexable(text: &str) -> &str {
    let mut end = text.len().min(MAX_INDEXED_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Plain-text snippet.
pub(super) fn preview(text: &str) -> String {
    shortened(text, PREVIEW_CHARS)
}

/// ADR 0032: the name is derived and never empty.
pub(super) fn display_name(subject: Option<&str>, text: &str) -> String {
    [subject.unwrap_or_default(), text]
        .iter()
        .map(|t| shortened(t, NAME_CHARS))
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "(no text)".to_string())
}
