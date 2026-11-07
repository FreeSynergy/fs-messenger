//! Names used in the store: object types and index keys.
//!
//! Pattern: none, allowed by section 2 (constants): the store layout in one place, see `DESIGN.md`.

pub(super) const NOTE: &str = "Note";
pub(super) const TOMBSTONE: &str = "Tombstone";
pub(super) const DOCUMENT: &str = "Document";
pub(super) const CONVERSATION: &str = "Collection";
pub(super) const DRAFT: &str = "freesynergy.net:Draft";
pub(super) const ACCOUNT: &str = "freesynergy.net:Account";
pub(super) const STORE_CHECK: &str = "freesynergy.net:StoreCheck";

pub(super) const CONTENT_TYPE: &str = "application/activity+json";
pub(super) const TEXT_PLAIN: &str = "text/plain";
pub(super) const TEXT_MARKDOWN: &str = "text/markdown";

/// ADR 0029: every object of this program carries it, so the store's own refusal holds for all of
/// them. DPV personal-data category; provisional, see `DESIGN.md`.
pub(super) const SENSITIVE: &str = "pd:Communication";

/// Local arrival order, first arrived first.
pub(super) const IDX_ARRIVAL: &str = "arrival";
/// Local arrival order, last arrived first.
pub(super) const IDX_ARRIVAL_DESC: &str = "arrival_desc";
/// Display order, newest first.
pub(super) const IDX_DESC: &str = "desc";
pub(super) const IDX_UNREAD: &str = "unread";
pub(super) const IDX_FLAGGED: &str = "flagged";
pub(super) const IDX_HAS_ATTACHMENT: &str = "attachment";
pub(super) const IDX_BLOB: &str = "blob";
pub(super) const IDX_FROM: &str = "from";
pub(super) const IDX_TEXT: &str = "text";
pub(super) const IDX_ALIAS: &str = "alias";
pub(super) const IDX_LINK: &str = "link";
pub(super) const IDX_RECEIPT: &str = "receipt";
pub(super) const IDX_REPLY_ALIAS: &str = "reply_alias";
pub(super) const IDX_ENCRYPTION: &str = "encryption";
pub(super) const IDX_KIND: &str = "kind";
pub(super) const IDX_JOB: &str = "job";
pub(super) const IDX_TASK: &str = "task";
pub(super) const IDX_ACCOUNT: &str = "account";
/// Conversation order: pinned first, then newest first.
pub(super) const IDX_SORT: &str = "sort";
pub(super) const IDX_FOLDER: &str = "folder";
pub(super) const IDX_CONV_ALIAS: &str = "conv_alias";
pub(super) const IDX_ADDRESS: &str = "address";
/// A message with a tool call that is not settled, so an emergency stop needs no scan.
pub(super) const IDX_OPEN_CALL: &str = "open_call";

pub(super) const YES: &str = "1";
